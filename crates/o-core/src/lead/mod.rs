//! The lead chat (ADR-0008): a person writes to a project's lead, an official CLI
//! reads the project (read-only) and proposes a work-card plan, and only the
//! person's "실행" makes O dispatch it, through the same policy as `o dispatch`.
//!
//! ```text
//! leads/current.json                 project (normalized) → its current thread id
//! leads/<threadId>/events.jsonl      the thread: lead.start · lead.user · lead.session · lead.reply
//!                                    · lead.plan · lead.dispatch · lead.usage · lead.warning · lead.error · lead.seen
//! leads/<threadId>/turn.raw.jsonl    the CLIs' stdout, for reading back a turn
//! leads/<threadId>/sessions.txt      the CLI session ids it used, one per line (`o runs --sessions` reads these)
//! leads/<threadId>/turn.lock         held (OS lock) by the turn in progress, for as long as it runs
//! leads/<threadId>/running.json      that turn's marker ({ provider, owner, token, child }); stop: its stop request
//! leads/<threadId>/dispatched/<planId>   made once, when a plan is dispatched
//! <base>/project_O-lead/<threadId>/  attachments (`files`), outside O's state folder
//! ```
//!
//! Lead events live in their own file with their own type ([`LeadEvent`]); run
//! events (`run::events`) are untouched. The lead's answer is data: its plan is
//! checked like any plan before it is stored, and again before it runs.

pub mod files;
pub mod log_page;
pub mod notes;
pub mod prompt;
pub mod recovery;
mod status_file;
mod turn_process;
#[cfg(windows)]
mod stream_status;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agents::clean;
use crate::context::normalize_project;
use crate::policy::{check_plan_for, check_project, is_model_id, Violation, LIMITS};
use crate::providers::{self, LeadTurn, StreamReader};
use crate::run::events::StreamSignal;
use crate::run::mode::{Profile, RunMode};
use crate::run::plan::ProviderId;
use crate::run::runner::Spawner;
use crate::run::team;
use crate::run::snapshot::{snapshot, RunStatus};
use crate::run::yolo::{self, AutoRunFacts, PastAutoRun};
use crate::run::store::{self, is_run_id, new_run_id, policy_context, read_events, record_blocked, run_dir};
use crate::sys::js::{js_len, js_tail, now_iso};
use crate::sys::proc;
use crate::usage::{budget, UsageWindow};

pub use files::Attachment;
type StartRun<'a> = dyn Fn(&str) -> io::Result<()> + 'a;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all_fields = "camelCase")]
pub enum LeadEvent {
    /// A new thread for a project ("새 대화" starts another).
    #[serde(rename = "lead.start")]
    Start { at: String, thread_id: String, project: String },
    #[serde(rename = "lead.user")]
    User {
        at: String,
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachments: Vec<Attachment>,
        provider: ProviderId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
        /// The profile this message ran on (project_O 프로필 사용, ADR-0013); absent in threads from before the switch.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<Profile>,
        /// The turn ran in YOLO 모드 (ADR-0014): the lead could edit files and run commands.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        yolo: bool,
    },
    /// Untrusted data sent by a card, never a user instruction or approval.
    #[serde(rename = "lead.worker")]
    Worker { at: String, run_id: String, card_id: String, message_id: String, text: String },
    /// The CLI's own session id, resumed on the thread's next turn with that CLI. `clean` is
    /// the profile it lives in: a turn on the other profile cannot resume it and starts a new one.
    /// Absent in threads from before the switch, where any turn resumes it.
    #[serde(rename = "lead.session")]
    Session {
        at: String,
        provider: ProviderId,
        session_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        clean: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result_tools: Option<bool>,
    },
    /// The lead's answer as Markdown, without its plan block.
    #[serde(rename = "lead.reply")]
    Reply { at: String, provider: ProviderId, text: String, #[serde(default, skip_serializing_if = "Vec::is_empty")] reported_files: Vec<String> },
    #[serde(rename = "lead.design")]
    Design { at: String, design: Value },
    /// A proposed plan with the project forced to the thread's and the policy check's result.
    #[serde(rename = "lead.plan")]
    Plan { at: String, plan_id: String, plan: Value, ok: bool, violations: Vec<Violation> },
    #[serde(rename = "lead.dispatch")]
    Dispatch {
        at: String,
        plan_id: String,
        run_id: String,
        clean: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<Profile>,
        /// The run's cards launch in YOLO 모드 (ADR-0014).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        yolo: bool,
    },
    #[serde(rename = "lead.followup")]
    Followup { at: String, source_run_id: String, card_id: String, run_id: String },
    #[serde(rename = "lead.autorun")]
    AutoRun { at: String, plan_id: String, hash: Option<String>, reason: Option<String> },
    /// Tokens one lead turn spent (`o tokens` counts them as planning overhead).
    #[serde(rename = "lead.usage")]
    Usage { at: String, provider: ProviderId, input: f64, cached: f64, output: f64, #[serde(default, skip_serializing_if = "Option::is_none")] complete: Option<bool> },
    #[serde(rename = "lead.warning")]
    Warning { at: String, text: String },
    #[serde(rename = "lead.error")]
    Error { at: String, text: String },
    /// A CLI session got the turn whose `lead.user` is event `through` of the thread (as
    /// [`read_thread`] counts): it has seen the thread up to that turn, its own answer included.
    #[serde(rename = "lead.seen")]
    Seen { at: String, provider: ProviderId, session_id: String, through: usize },
}

fn leads_dir(home: &Path) -> PathBuf {
    home.join("leads")
}

pub fn thread_dir(home: &Path, thread_id: &str) -> PathBuf {
    leads_dir(home).join(thread_id)
}

/// The key a project's thread is filed under: `/` slashes, no trailing one, and on Windows any case.
fn project_key(project: &str) -> String {
    let key = normalize_project(project);
    if cfg!(windows) { key.to_lowercase() } else { key }
}

/// Every project's current thread. Missing is empty; a list that cannot be read is an error,
/// never "empty" (that would start new threads over every project's). It is replaced whole
/// (`write_current`), so a read that fails is tried again only briefly.
fn read_current(home: &Path) -> io::Result<BTreeMap<String, String>> {
    let path = leads_dir(home).join("current.json");
    let mut tries = 0;
    loop {
        let read = match fs::read_to_string(&path) {
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(BTreeMap::new()),
            read => read.and_then(|t| serde_json::from_str(&t).map_err(io::Error::other)),
        };
        match read {
            Ok(current) => return Ok(current),
            Err(_) if tries < 3 => tries += 1,
            Err(e) => return Err(io::Error::other(format!("총괄 대화 목록({})을 읽지 못했습니다: {e}", path.to_string_lossy()))),
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// Replaces the list whole: a reader sees the old list or the new one, never a part.
fn write_current(home: &Path, current: &BTreeMap<String, String>) -> io::Result<()> {
    let path = leads_dir(home).join("current.json");
    let temp = path.with_extension(format!("{}.tmp", new_run_id(Utc::now())));
    fs::write(&temp, store::to_pretty(current)?)?;
    fs::rename(&temp, &path)
}

/// The project's current thread, if it has one (none when the list cannot be read).
pub fn current_thread(home: &Path, project: &str) -> Option<String> {
    read_current(home).ok()?.get(&project_key(project)).filter(|id| is_run_id(id)).cloned()
}

/// Starts a new thread for the project and makes it the current one.
pub fn new_thread(home: &Path, project: &str) -> io::Result<String> {
    thread_for(home, project, true)
}

/// The project's current thread, or a new one when it has none or `fresh` asks for it.
/// Changes to the list are made one at a time (an OS lock on `leads/current.lock`), so two
/// first messages make one thread and threads started together for other projects all stay.
pub fn thread_for(home: &Path, project: &str, fresh: bool) -> io::Result<String> {
    fs::create_dir_all(leads_dir(home))?;
    let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(leads_dir(home).join("current.lock"))?;
    lock.lock()?;
    let mut current = read_current(home)?;
    let key = project_key(project);
    if let Some(id) = current.get(&key).filter(|id| !fresh && is_run_id(id)) {
        return Ok(id.clone());
    }
    let thread_id = new_run_id(Utc::now());
    fs::create_dir_all(thread_dir(home, &thread_id))?;
    append(home, &thread_id, &LeadEvent::Start { at: now_iso(), thread_id: thread_id.clone(), project: project.to_string() })?;
    fs::write(sessions_path(home, &thread_id), "")?;
    current.insert(key, thread_id.clone());
    write_current(home, &current)?;
    Ok(thread_id)
}

fn append(home: &Path, thread_id: &str, event: &LeadEvent) -> io::Result<()> {
    let line = serde_json::to_string(event).map_err(io::Error::other)?;
    let mut file = OpenOptions::new().create(true).append(true).open(thread_dir(home, thread_id).join("events.jsonl"))?;
    file.write_all(format!("{line}\n").as_bytes())
}

fn event_values(home: &Path, thread_id: &str) -> Vec<Value> {
    let text = fs::read(thread_dir(home, thread_id).join("events.jsonl")).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    store::parse_event_values(&text)
}

/// The thread's events; lines this build does not know are skipped.
pub fn read_thread(home: &Path, thread_id: &str) -> Vec<LeadEvent> {
    event_values(home, thread_id).into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect()
}

fn thread_project(events: &[LeadEvent]) -> Option<String> {
    events.iter().find_map(|e| match e {
        LeadEvent::Start { project, .. } => Some(project.clone()),
        _ => None,
    })
}

/// What a turn's owner says about it in `running.json` (written whole, by rename).
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Running {
    provider: ProviderId,
    /// The process that holds the turn (the app, or `o lead send`).
    owner: u32,
    /// This turn's own id, so a turn never removes another turn's marker.
    token: String,
    /// The lead CLI, once it started. Only this is ever ended by a stop, never the owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    child: Option<ChildMark>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unresolved: Option<bool>,
}

/// A started lead CLI: its pid and start time (`proc::started_at`), which together name
/// that one process even after the pid is handed to another.
#[derive(Clone, Debug, Deserialize, Serialize)]
struct ChildMark {
    pid: u32,
    #[serde(default)]
    started: Option<u64>,
}

/// A turn holding a thread: its marker, or `None` while the owner has not written it yet.
struct Busy {
    marker: Option<Running>,
}

/// `busy` of a turn whose owner has not written its marker yet.
const STARTING: &str = "starting";

/// How often a running turn looks for a stop request.
const STOP_POLL: Duration = Duration::from_millis(100);

const BUSY: &str = "총괄이 아직 답하는 중입니다. 끝나거나 중지한 뒤 보내세요";

fn running_path(home: &Path, thread_id: &str) -> PathBuf {
    thread_dir(home, thread_id).join("running.json")
}

fn stop_path(home: &Path, thread_id: &str) -> PathBuf {
    thread_dir(home, thread_id).join("stop")
}

fn read_marker(home: &Path, thread_id: &str) -> Option<Running> {
    fs::read_to_string(running_path(home, thread_id)).ok().and_then(|t| serde_json::from_str(&t).ok())
}

/// The file a turn holds an OS lock on for as long as it runs. The OS lets go of the lock
/// when the owning process ends, however it ends, so a crashed turn holds nothing.
fn turn_lock(home: &Path, thread_id: &str) -> io::Result<fs::File> {
    OpenOptions::new().read(true).write(true).create(true).truncate(false).open(thread_dir(home, thread_id).join("turn.lock"))
}

/// The live turn of a thread, if any. Whether a turn runs is the lock, never the marker:
/// a marker that is missing, empty or half-written under a held lock is a turn starting,
/// and a marker without a held lock is a turn whose process is gone.
fn running(home: &Path, thread_id: &str) -> Option<Busy> {
    let lock = turn_lock(home, thread_id).ok()?;
    match lock.try_lock() {
        // Free: no turn. Dropping the file lets go of the lock again.
        Ok(()) => return None,
        Err(fs::TryLockError::WouldBlock) => {}
        Err(fs::TryLockError::Error(_)) => return None,
    }
    Some(Busy { marker: read_marker(home, thread_id) })
}

/// Holds a thread's one turn: an OS lock on `turn.lock` that only one holder gets, and the
/// marker beside it. Dropped, it removes its marker and lets go of the lock.
struct TurnSlot {
    _lock: fs::File,
    path: PathBuf,
    stop: PathBuf,
    marker: Running,
    keep_marker: bool,
}

impl TurnSlot {
    fn take(home: &Path, thread_id: &str, provider: ProviderId) -> Result<TurnSlot, String> {
        let lock = turn_lock(home, thread_id).map_err(|e| e.to_string())?;
        // `show` holds the lock for an instant when it looks; only a turn holds it longer.
        let deadline = Instant::now() + Duration::from_millis(300);
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(fs::TryLockError::WouldBlock) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                Err(fs::TryLockError::WouldBlock) => return Err(BUSY.into()),
                Err(fs::TryLockError::Error(e)) => return Err(format!("총괄 대화를 시작하지 못했습니다: {e}")),
            }
        }
        if read_marker(home, thread_id).is_some_and(|marker| marker.unresolved == Some(true)) {
            return Err("이전 총괄의 종료 상태가 불명입니다. 프로세스와 출력을 확인하세요".into());
        }
        // A stop file here is an older turn's: this one has no marker yet, so none was meant for it.
        let _ = fs::remove_file(stop_path(home, thread_id));
        let marker = Running { provider, owner: std::process::id(), token: new_run_id(Utc::now()), child: None, unresolved: None };
        let slot = TurnSlot { _lock: lock, path: running_path(home, thread_id), stop: stop_path(home, thread_id), marker, keep_marker: false };
        slot.write().map_err(|e| format!("총괄 대화를 시작하지 못했습니다: {e}"))?;
        Ok(slot)
    }

    /// Writes the marker whole: a reader sees the old one or the new one, never a part.
    fn write(&self) -> io::Result<()> {
        let temp = self.path.with_extension(format!("{}.tmp", self.marker.token));
        fs::write(&temp, serde_json::to_string(&self.marker).map_err(io::Error::other)?)?;
        fs::rename(&temp, &self.path)
    }

    fn set_child(&mut self, pid: u32) {
        self.marker.child = Some(ChildMark { pid, started: proc::started_at(pid) });
        let _ = self.write();
    }

    /// Whether this turn was asked to stop: the stop file names its token, or is empty (written
    /// by a stop that could not read the marker, which can only be this turn's, since a turn
    /// clears any older stop before it writes its marker).
    fn stop_asked(&self) -> bool {
        fs::read_to_string(&self.stop).is_ok_and(|t| t.is_empty() || t == self.marker.token)
    }
}

impl Drop for TurnSlot {
    fn drop(&mut self) {
        // An unresolved marker must outlive the lock; its stop request must not.
        if !self.keep_marker { let _ = fs::remove_file(&self.path); }
        let _ = fs::remove_file(&self.stop);
    }
}

/// One message to the lead.
pub struct SendRequest<'a> {
    pub project: &'a str,
    pub provider: ProviderId,
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    pub text: &'a str,
    /// Files to attach: saved ones of this thread, or any file to copy in (`files::import`).
    pub attach: &'a [PathBuf],
    /// Start a new thread first.
    pub fresh: bool,
    /// The clean profile root when the lead runs on the project_O profile (ADR-0013); None is the person's own setup.
    pub clean: Option<&'a Path>,
    /// YOLO 모드 (ADR-0014): the lead may edit files and run commands (Provider::yolo_lead).
    pub yolo: bool,
}

/// Checks a request before anything is saved or started.
pub fn check_request(home: &Path, req: &SendRequest) -> Result<(), String> {
    if req.provider == ProviderId::Agy { return Err(crate::policy::AGY_UNAVAILABLE.into()); }
    let provider = providers::get(req.provider);
    if provider.lead_efforts().is_empty() {
        return Err(format!("{}는 총괄로 쓸 수 없습니다 (읽기 전용 실행과 이어 말하기를 보장하지 못함)", req.provider));
    }
    if let Some(effort) = req.effort.filter(|e| !e.is_empty()) {
        if !provider.lead_efforts().contains(&effort) {
            return Err(format!("{} 총괄의 effort는 {} 중 하나입니다", req.provider, provider.lead_efforts().join(", ")));
        }
    }
    if let Some(model) = req.model.filter(|m| !m.is_empty()) {
        if !is_model_id(model) {
            return Err("model 이름 형식이 올바르지 않습니다".into());
        }
    }
    if req.text.trim().is_empty() {
        return Err("총괄에게 보낼 내용이 비어 있습니다".into());
    }
    if js_len(req.text) > LIMITS.max_task_chars {
        return Err(format!("메시지는 {}자 이내입니다", LIMITS.max_task_chars));
    }
    if req.attach.len() > files::MAX_PER_MESSAGE {
        return Err(format!("첨부는 메시지당 {}개까지입니다", files::MAX_PER_MESSAGE));
    }
    // agy runs a project's `.agents/hooks.json` (antigravity.google/docs/hooks) and has no
    // documented switch to turn hooks off for one run, so it does not lead such a project.
    let agy_hooks = Path::new(req.project).join(".agents").join("hooks.json");
    if req.provider == ProviderId::Agy && agy_hooks.exists() {
        return Err(format!("이 프로젝트에 {}이 있어 Antigravity를 총괄로 쓰지 않습니다 (계획 승인 전에 프로젝트 훅이 실행될 수 있음). Claude Code나 Codex를 고르세요", agy_hooks.to_string_lossy()));
    }
    check_folder(home, req.project)
}

/// Whether a folder may have a lead: the lead reads its project, so the folders a card
/// may not work in (O's state folder, the home folder itself, mirrors) are refused.
pub fn check_folder(home: &Path, project: &str) -> Result<(), String> {
    let ctx = policy_context(home, Utc::now()).map_err(|e| e.to_string())?;
    match check_project(Some(project), &ctx).first() {
        Some(v) => Err(v.message.clone()),
        None => Ok(()),
    }
}

/// "새 대화" or a first folder: a new thread for a folder that may have a lead.
pub fn start(home: &Path, project: &str) -> Result<String, String> {
    check_folder(home, project)?;
    new_thread(home, project).map_err(|e| e.to_string())
}

// Bound the number of separate run sources and index rows held for one turn.
const MAX_STATUS_RUNS: usize = 128;
#[cfg(windows)]
const MAX_STATUS_RECORDS: usize = 512;

fn dispatched_runs(events: &[LeadEvent]) -> (Vec<String>, bool) {
    let mut seen = BTreeSet::new();
    let mut runs = Vec::new();
    let mut uncertain = false;
    for event in events {
        let run = match event {
            LeadEvent::Dispatch { run_id, .. } | LeadEvent::Followup { run_id, .. } => run_id,
            _ => continue,
        };
        if !seen.insert(run) { continue; }
        if !is_run_id(run) { uncertain = true; continue; }
        if runs.len() == MAX_STATUS_RUNS { return (Vec::new(), true); }
        runs.push(run.clone());
    }
    (runs, uncertain)
}

/// The messages a resumed session has not seen: those of every turn after the last one it
/// got (`lead.seen`; a thread written before that event counts its last turn to this agent).
fn missed_turns(events: &[LeadEvent], provider: ProviderId, session_id: &str) -> Vec<(String, String)> {
    let seen = events.iter().rev().find_map(|e| match e {
        LeadEvent::Seen { provider: p, session_id: s, through, .. } if *p == provider && s == session_id => Some(*through),
        _ => None,
    });
    let seen = seen.or_else(|| events.iter().rposition(|e| matches!(e, LeadEvent::User { provider: p, .. } if *p == provider)));
    let Some(seen) = seen else { return Vec::new() };
    // Its own turn's answer follows its message; what it missed starts at the next message.
    let Some(next) = events.iter().skip(seen + 1).position(|e| matches!(e, LeadEvent::User { .. } | LeadEvent::Worker { .. })) else { return Vec::new() };
    events[seen + 1 + next..]
        .iter()
        .filter_map(|e| match e {
            LeadEvent::User { text, .. } => Some(("사용자".to_string(), text.clone())),
            LeadEvent::Worker { card_id, text, .. } => Some((format!("카드({card_id}) 참고 데이터"), text.clone())),
            LeadEvent::Reply { text, provider, .. } => Some((format!("총괄({provider})"), text.clone())),
            _ => None,
        })
        .collect()
}

fn budget_lines(home: &Path) -> Vec<String> {
    budget::report_now(home).map(|r| r.providers.into_iter().map(|p| p.message).filter(|m| !m.is_empty()).collect()).unwrap_or_default()
}

// shortcut: hide all code fences until completion, including ordinary code; upgrade when previews parse streamed Markdown.
fn partial_prose(text: &str) -> &str {
    text.find("```").map_or_else(|| text.trim_end_matches('`'), |at| &text[..at])
}

#[cfg(test)]
mod preview_tests {
    use super::partial_prose;

    #[test]
    fn partial_fences_never_expose_their_contents() {
        for text in ["답변\n`", "답변\n``", "답변\n```o-plan\n비공개"] {
            assert_eq!(partial_prose(text), "답변\n");
        }
        assert_eq!(partial_prose("아직 답변 중"), "아직 답변 중");
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;

    #[test]
    fn dispatched_runs_keep_order_deduplicate_and_limit() {
        let event = |id: &str| LeadEvent::Dispatch { at: "now".into(), plan_id: "p".into(), run_id: id.into(), clean: false, profile: None, yolo: false };
        let (runs, uncertain) = dispatched_runs(&[event("b"), event("a"), event("b"), event("../bad")]);
        assert_eq!(runs, ["b", "a"]);
        assert!(uncertain);
        let events: Vec<_> = (0..=MAX_STATUS_RUNS).map(|n| event(&format!("run-{n}"))).collect();
        let (runs, uncertain) = dispatched_runs(&events);
        assert!(runs.is_empty() && uncertain);
    }
}

/// Sends one message: saves it and its attachments, runs the lead CLI read-only in the
/// project (resuming its session), and records the reply, the plan with its policy
/// result, the tokens and any error. Returns the events this turn added.
pub fn send(home: &Path, req: &SendRequest, spawner: &Spawner) -> Result<Vec<LeadEvent>, String> {
    send_within(home, req, spawner, TURN_LIMIT)
}

/// How long one lead turn may run before its CLI is ended (the dev server waited as long).
pub const TURN_LIMIT: Duration = Duration::from_secs(15 * 60);

/// [`send`] with its own time limit for the CLI.
pub fn send_within(home: &Path, req: &SendRequest, spawner: &Spawner, limit: Duration) -> Result<Vec<LeadEvent>, String> {
    send_within_followups(home, req, spawner, limit, None)
}

pub fn send_with_followups(home: &Path, req: &SendRequest, spawner: &Spawner, start: &dyn Fn(&str) -> io::Result<()>) -> Result<Vec<LeadEvent>, String> {
    send_within_followups(home, req, spawner, TURN_LIMIT, Some(start))
}

fn send_within_followups(home: &Path, req: &SendRequest, spawner: &Spawner, limit: Duration, start: Option<&StartRun<'_>>) -> Result<Vec<LeadEvent>, String> {
    send_turn(home, req, spawner, limit, start, None)
}

struct WorkerTurn<'a> {
    thread_id: &'a str,
    speaker: &'a team::Speaker,
    message: &'a team::Received,
}

/// Handle one queued card message for the current thread that dispatched this run.
/// `false` means there is no eligible message; unknown deliveries require human recovery.
pub fn wake_card(home: &Path, project: &str, run_id: &str, cards: &[String], spawner: &Spawner) -> Result<bool, String> {
    let Some(thread_id) = current_thread(home, project) else { return Ok(false) };
    let events = read_thread(home, &thread_id);
    let Some(dispatch) = events.iter().rposition(|event| matches!(event,
        LeadEvent::Dispatch { run_id: id, .. } | LeadEvent::Followup { run_id: id, .. } if id == run_id)) else { return Ok(false) };
    let Some((provider, model, effort, profile)) = events[..dispatch].iter().rev().find_map(|event| match event {
        LeadEvent::User { provider, model, effort, profile, .. } => Some((*provider, model.as_deref(), effort.as_deref(), *profile)),
        _ => None,
    }) else { return Err("실행에 연결된 총괄 설정이 없습니다".into()) };
    let Some(profile) = profile else { return Err("이전 총괄의 프로필이 기록되지 않아 자동 응답할 수 없습니다".into()) };
    let speaker = team::Speaker { run_id: run_id.into(), me: "lead".into(), cards: cards.to_vec() };
    let Some(message) = team::read(home, &speaker, false).map_err(|e| e.to_string())?.into_iter().next() else { return Ok(false) };
    if message.kind == "approval" {
        let answered = team::read(home, &speaker, true).map_err(|e| e.to_string())?.iter().any(|m| m.from == "lead" && m.to == message.from && m.reply_to.as_deref() == Some(&message.id));
        if answered {
            team::transition(home, &speaker, &message.id, "acknowledged").map_err(|e| e.to_string())?;
            return Ok(true);
        }
        return Err("승인 요청은 대표님의 답변을 기다립니다".into());
    }
    if message.state != "stored" { return Err("카드 메시지의 기존 전달 상태를 확인하세요".into()); }
    let root = clean::default_root();
    let req = SendRequest { project, provider, model, effort, text: &message.text, attach: &[], fresh: false, clean: profile.is_clean().then_some(root.as_path()), yolo: false };
    send_turn(home, &req, spawner, TURN_LIMIT, None, Some(WorkerTurn { thread_id: &thread_id, speaker: &speaker, message: &message }))?;
    if !team::read(home, &speaker, true).map_err(|e| e.to_string())?.iter().any(|m| m.id == message.id && m.state == "acknowledged") {
        return Err("총괄 처리 결과가 불명입니다. 대화와 메시지 상태를 확인하세요".into());
    }
    Ok(true)
}

/// Explicit recovery for a queued message after its runner has stopped.
pub fn wake_run(home: &Path, run_id: &str, spawner: &Spawner) -> Result<bool, String> {
    if !is_run_id(run_id) { return Err("실행 ID가 올바르지 않습니다".into()); }
    let value = store::read_plan(home, run_id).map_err(|e| e.to_string())?;
    let plan: crate::run::plan::Plan = serde_json::from_value(value).map_err(|e| e.to_string())?;
    let cards = plan.cards.iter().map(|card| card.id.clone()).collect::<Vec<_>>();
    wake_card(home, &plan.project, run_id, &cards, spawner)
}

/// Reconcile an uncertain delivery, or put it back only after an explicit user request.
pub fn retry_card(home: &Path, run_id: &str, message_id: &str) -> Result<&'static str, String> {
    if !is_run_id(run_id) { return Err("실행 ID가 올바르지 않습니다".into()); }
    let plan: crate::run::plan::Plan = serde_json::from_value(store::read_plan(home, run_id).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let current = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(leads_dir(home).join("current.lock")).map_err(|e| e.to_string())?;
    current.lock().map_err(|e| e.to_string())?;
    let thread_id = current_thread(home, &plan.project).ok_or("현재 총괄 대화가 없습니다")?;
    let turn = turn_lock(home, &thread_id).map_err(|e| e.to_string())?;
    turn.try_lock().map_err(|_| "총괄이 응답 중입니다. 완료 후 상태를 확인하세요")?;
    if !read_thread(home, &thread_id).iter().any(|e| matches!(e, LeadEvent::Dispatch { run_id: id, .. } | LeadEvent::Followup { run_id: id, .. } if id == run_id)) {
        return Err("현재 총괄 대화가 분배한 실행이 아닙니다".into());
    }
    let speaker = team::Speaker { run_id: run_id.into(), me: "lead".into(), cards: plan.cards.iter().map(|card| card.id.clone()).collect() };
    let pending = team::read(home, &speaker, false).map_err(|e| e.to_string())?;
    let message = pending.first().filter(|m| m.id == message_id && matches!(m.state.as_str(), "unknown" | "delivered")).ok_or("첫 번째 처리 불명 메시지를 확인하세요")?;
    let sent = team::read(home, &speaker, true).map_err(|e| e.to_string())?.iter().any(|m| m.from == "lead" && m.to == message.from && m.reply_to.as_deref() == Some(message_id));
    let state = if sent { "acknowledged" } else { "stored" };
    team::transition(home, &speaker, message_id, state).map_err(|e| e.to_string())?;
    Ok(if sent { "acknowledged" } else { "queued" })
}

fn send_turn(home: &Path, req: &SendRequest, spawner: &Spawner, limit: Duration, start: Option<&StartRun<'_>>, worker: Option<WorkerTurn<'_>>) -> Result<Vec<LeadEvent>, String> {
    let turn_started = Utc::now();
    check_request(home, req)?;
    let thread_id = match &worker {
        Some(w) => w.thread_id.to_string(),
        None => thread_for(home, req.project, req.fresh).map_err(|e| e.to_string())?,
    };
    let mut slot = TurnSlot::take(home, &thread_id, req.provider)?;
    if let Some(w) = &worker {
        if req.yolo || store::stop_requested(home, &w.speaker.run_id) || current_thread(home, req.project).as_deref() != Some(&thread_id)
            || !read_thread(home, &thread_id).iter().any(|e| matches!(e, LeadEvent::Dispatch { run_id, .. } | LeadEvent::Followup { run_id, .. } if run_id == &w.speaker.run_id)) {
            return Err("실행과 현재 총괄 대화가 일치하지 않습니다".into());
        }
        let claimed = team::claim(home, w.speaker).map_err(|e| e.to_string())?;
        if claimed.as_ref().is_none_or(|m| m.id != w.message.id) { return Err("다른 메시지가 먼저 접수되어 상태 확인이 필요합니다".into()); }
    }
    let pending: Vec<_> = if worker.is_some() { Vec::new() } else { notes::read(home, &thread_id).map_err(|e| format!("설계 주석을 읽지 못했습니다: {e}"))?.into_iter().filter(|note| !note.sent).collect() };
    let mut user_text = req.text.trim().to_string();
    if !pending.is_empty() {
        user_text.push_str("\n\n[설계에 남긴 주석]\n");
        for note in &pending { user_text.push_str(&format!("- [{}] “{}”: {}\n", note.tag, note.quote, note.text)); }
        if js_len(&user_text) > LIMITS.max_task_chars { return Err("메시지와 주석을 합친 길이가 너무 깁니다. 주석을 줄인 뒤 다시 보내세요".into()); }
    }
    let progress_path = thread_dir(home, &thread_id).join("progress.json");
    let _ = fs::write(&progress_path, "[]");
    let before = read_thread(home, &thread_id);
    let attachments = req.attach.iter().map(|path| files::import(home, &thread_id, path)).collect::<Result<Vec<_>, _>>()?;

    let mut added = Vec::new();
    let mut emit = |event: LeadEvent| -> Result<(), String> {
        append(home, &thread_id, &event).map_err(|e| e.to_string())?;
        added.push(event);
        Ok(())
    };
    if let Some(w) = &worker {
        emit(LeadEvent::Worker { at: now_iso(), run_id: w.speaker.run_id.clone(), card_id: w.message.from.clone(), message_id: w.message.id.clone(), text: user_text.clone() })?;
    } else { emit(LeadEvent::User {
        at: now_iso(),
        text: user_text.clone(),
        attachments: attachments.clone(),
        provider: req.provider,
        model: req.model.filter(|m| !m.is_empty()).map(str::to_string),
        effort: req.effort.filter(|e| !e.is_empty()).map(str::to_string),
        profile: Some(Profile::of_clean(req.clean.is_some())),
        yolo: req.yolo,
    })?; }

    let previous_session = if worker.is_some() { None } else { before.iter().rev().find_map(|e| match e {
        LeadEvent::Session { provider, session_id, clean, result_tools, .. } if *provider == req.provider && clean.is_none_or(|c| c == req.clean.is_some()) => {
            Some((session_id.clone(), *result_tools))
        }
        _ => None,
    }) };
    let history: Vec<(String, String)> = before
        .iter()
        .filter_map(|e| match e {
            LeadEvent::User { text, .. } => Some(("사용자".to_string(), text.clone())),
            LeadEvent::Worker { card_id, text, .. } => Some((format!("카드({card_id}) 참고 데이터"), text.clone())),
            LeadEvent::Reply { text, .. } => Some(("총괄".to_string(), text.clone())),
            _ => None,
        })
        .collect();
    let (ids, invalid) = dispatched_runs(&before);
    let mut runs = if invalid { vec!["- 실행 ID 또는 실행 수가 올바르지 않아 일부 상태 불명".to_string()] } else { Vec::new() };
    let mut status_error = invalid.then(|| "일부 실행의 ID 또는 실행 수를 확인할 수 없습니다".to_string());
    #[cfg(windows)]
    let status_file = if ids.is_empty() { None } else {
        match stream_status::write(home, &thread_id, &ids) {
            Ok((file, summaries, uncertain, first_error)) => {
                runs.extend(summaries);
                if uncertain > 0 { status_error = Some(first_error.map_or_else(|| "일부 실행은 기록 접근 실패, 안전 상한 또는 해석하지 못한 기록으로 상태가 불명입니다".to_string(), |error| format!("일부 실행의 상태가 불명입니다: {error}"))); }
                Some(file)
            }
            Err(e) => {
                runs.extend(ids.iter().map(|id| format!("- 실행 {id}: 상태 불명")));
                status_error = Some(format!("실행 결과 원문을 안전하게 제공할 수 없습니다: {e}"));
                None
            }
        }
    };
    #[cfg(not(windows))]
    let status_file: Option<status_file::StatusFile> = if ids.is_empty() { None } else {
        runs.extend(ids.iter().map(|id| match snapshot(&read_events(home, id)) {
            Some(view) => prompt::run_status(&view),
            None => format!("- 실행 {id}: 아직 기록 없음 또는 상태 불명"),
        }));
        status_error = Some("이 운영체제에서는 실행 결과 원문 파일을 지원하지 않아 요약만 제공됩니다".to_string());
        None
    };
    if let Some(error) = &status_error { emit(LeadEvent::Warning { at: now_iso(), text: format!("실행 상태 확인 불가: {error}") })?; }
    let app_server = req.provider == ProviderId::Codex
        && !req.yolo
        && spawner(&providers::codex::app_server_probe(), req.project).is_ok_and(providers::codex::supports_app_server);
    let migrate = app_server && status_file.is_some() && previous_session.as_ref().is_some_and(|(_, tools)| *tools != Some(true));
    if migrate {
        emit(LeadEvent::Warning { at: now_iso(), text: "결과 읽기 도구를 사용하려고 Codex 세션을 새로 시작합니다. 이전 대화는 최근 일부만 전달되며 캐시가 초기화될 수 있습니다.".into() })?;
    }
    let result_tools = cfg!(windows) && app_server && (previous_session.is_none() || migrate || previous_session.as_ref().is_some_and(|(_, tools)| *tools == Some(true)));
    let session = if migrate { None } else { previous_session.map(|(id, _)| id) };
    let missed = session.as_deref().map(|id| missed_turns(&before, req.provider, id)).unwrap_or_default();
    let status_path = status_file.as_ref().map(|f| f.path.to_string_lossy().into_owned());
    let budget = budget_lines(home);
    let design = before.iter().rev().find_map(|event| match event { LeadEvent::Design { design, .. } => serde_json::to_string(design).ok(), _ => None });
    let input = prompt::PromptInput {
        project: req.project,
        text: &user_text,
        attachments: &attachments,
        budget: &budget,
        runs: &runs,
        runs_file: status_path.as_deref(),
        runs_error: status_error.as_deref(),
        result_tools,
        resumed: session.is_some(),
        history: &history,
        missed: &missed,
        design: design.as_deref(),
        yolo: req.yolo,
    };
    let text = if worker.is_some() { prompt::build_worker(&input) } else { prompt::build(&input) };
    let files_dir = status_file.as_ref().and_then(|f| f.path.parent()).map(Path::to_path_buf).unwrap_or_else(|| files::thread_files(home, &thread_id));
    let files_dir = files_dir.exists().then(|| files_dir.to_string_lossy().into_owned());
    let images: Vec<String> = attachments.iter().filter(|a| a.image).map(|a| a.path.clone()).collect();
    let turn = LeadTurn {
        project: req.project,
        prompt: text,
        session: session.as_deref(),
        model: req.model.filter(|m| !m.is_empty()),
        effort: req.effort.filter(|e| !e.is_empty()),
        files_dir: files_dir.as_deref(),
        images: &images,
        app_server,
    };
    let Some(spec) = providers::lead_spec(req.provider, &turn, req.yolo) else {
        return Err(format!("{}는 총괄로 쓸 수 없습니다", req.provider));
    };

    let dir = thread_dir(home, &thread_id);
    let stopped = || LeadEvent::Error { at: now_iso(), text: "총괄을 중지했습니다".into() };
    let command = spawner(&spec, req.project).and_then(|mut command| {
        if let Some(root) = req.clean {
            // ADR-0015: a lead on the project_O profile needs that profile signed in.
            if let Some(why) = clean::not_ready(root, &[req.provider]).remove(&req.provider) {
                return Err(io::Error::other(why));
            }
            let env = clean::environment(root, req.provider)?;
            command.env_clear().envs(env);
        }
        // An inherited API key would bill the API instead of the subscription the CLI signed in with.
        providers::remove_inherited(&mut command, &spec.env_remove);
        command.envs(&spec.env);
        command.env("O_AGENT_ROLE", "lead");
        Ok(command)
    });
    // A stop that came while the turn set up (attachments, budget, the command) ends it here,
    // before the CLI can spend anything.
    if slot.stop_asked() {
        emit(stopped())?;
        return Ok(added);
    }
    let spawned = command.and_then(|mut command| proc::hide_window(&mut command).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn());
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            emit(LeadEvent::Error { at: now_iso(), text: format!("총괄 CLI를 시작하지 못했습니다: {error}") })?;
            return Ok(added);
        }
    };
    #[cfg(not(test))]
    let tree = proc::contain(&child);
    #[cfg(test)]
    let tree = if turn_process::NO_JOB.get() { None } else { proc::contain(&child) };
    let contained = tree.is_some();
    slot.set_child(child.id());
    let mut stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let process = turn_process::TurnProcess::start(child, tree, slot.stop.clone(), slot.marker.token.clone(), limit);
    let mut stdin_worker = None;
    let mut server_stdin = None;
    if app_server {
        if let Some(stdin) = stdin.take() {
            let (writer, worker) = process.writer(stdin);
            server_stdin = Some(writer);
            stdin_worker = Some(worker);
        }
    } else if let Some(mut stdin) = stdin.take() {
        let text = spec.stdin.clone().unwrap_or_default();
        stdin_worker = Some(thread::spawn(move || {
            let _ = stdin.write_all(text.as_bytes());
        }));
    }
    let stderr_tail = Arc::new(Mutex::new(String::new()));
    let stderr = stderr_pipe.map(|mut stderr| {
        let tail = Arc::clone(&stderr_tail);
        thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            while let Ok(n) = stderr.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let mut tail = tail.lock().unwrap();
                tail.push_str(&String::from_utf8_lossy(&chunk[..n]));
                *tail = js_tail(&tail, 400).to_string();
            }
        })
    });
    let (stdout, stdout_worker) = match stdout {
        Some(stdout) => { let (reader, worker) = process.reader(stdout); (Some(reader), Some(worker)) }
        None => (None, None),
    };

    let mut reader = StreamReader::new(req.provider);
    // The session this turn ran in: the resumed one, or the one the stream names.
    let mut used_session = if app_server { None } else { session.clone() };
    let (mut input, mut cached, mut output) = (0.0, 0.0, 0.0);
    let mut used = false;
    let mut usage_complete = true;
    let mut last_message = String::new();
    let mut last: Option<(bool, String)> = None;
    let mut limited = false;
    let mut progress = Vec::<Value>::new();
    let mut counts = [0u64; 4];
    let mut partial = String::new();
    let mut partial_written = None::<Instant>;
    let mut reported_files = BTreeSet::new();
    // An error that ends the turn after cleanup, and whether app-server finished the turn successfully.
    let mut fatal = None::<String>;
    let mut finished = false;
    let mut provider_finished = false;
    let mut raw_issue = None::<String>;
    if let Some(stdout) = stdout {
        let mut raw = match OpenOptions::new().create(true).append(true).open(dir.join("turn.raw.jsonl")) {
            Ok(file) => Some(file),
            Err(error) => { raw_issue = Some(error.to_string()); None }
        };
        let mut process_line = |line: &str| -> Result<(), String> {
            if let Ok(value) = serde_json::from_str::<Value>(line) {
                if value["type"] == "o.delta" {
                    if let Some(delta) = value["delta"].as_str() {
                        // shortcut: previews stop at 64 KiB; upgrade if longer live previews are needed.
                        for ch in delta.chars() {
                            if partial.len() + ch.len_utf8() > 65_536 { break; }
                            partial.push(ch);
                        }
                        if partial_written.is_none_or(|at| at.elapsed() >= Duration::from_millis(100)) {
                            let shown = partial_prose(&partial);
                            let state = json!({ "steps": progress, "counts": { "reads": counts[0], "searches": counts[1], "edits": counts[2], "commands": counts[3] }, "partial": shown });
                            if let Ok(text) = serde_json::to_string(&state) { let _ = fs::write(&progress_path, text); }
                            partial_written = Some(Instant::now());
                        }
                    }
                    return Ok(());
                }
                if value["type"] == "item.completed" && value["item"]["type"] == "agent_message" {
                    partial.clear();
                    partial_written = None;
                }
                if req.provider == ProviderId::Codex && value["type"] == "item.completed" && value["item"]["type"] == "file_change" && value["item"]["status"] != "failed" && value["item"]["status"] != "declined" {
                    if let Some(changes) = value["item"]["changes"].as_array() {
                        for path in changes.iter().filter_map(|change| change["path"].as_str()).take(100) {
                            if reported_files.len() < 100 && !path.is_empty() { reported_files.insert(path.chars().take(1024).collect::<String>()); }
                        }
                    }
                }
            }
            if let Some(file) = raw.as_mut() {
                if let Err(error) = file.write_all(format!("{line}\n").as_bytes()) {
                    raw_issue = Some(error.to_string());
                    raw = None;
                }
            }
            for signal in reader.read(line) {
                match signal {
                    StreamSignal::Session { session_id } => {
                        if worker.is_none() && session.as_deref() != Some(session_id.as_str()) {
                            emit(LeadEvent::Session { at: now_iso(), provider: req.provider, session_id: session_id.clone(), clean: Some(req.clean.is_some()), result_tools: result_tools.then_some(true) })?;
                            index_session(home, &thread_id, &session_id);
                        }
                        if worker.is_some() { index_session(home, &thread_id, &session_id); }
                        used_session = Some(session_id);
                    }
                    StreamSignal::Message { text } => last_message = text,
                    StreamSignal::Usage { input: i, cached: c, output: o, complete } => {
                        (input, cached, output, used) = (input + i, cached + c, output + o, true);
                        usage_complete &= complete;
                    }
                    StreamSignal::Limit { until } => {
                        limited = true;
                        let _ = store::set_cooldown(home, req.provider, &until, Utc::now());
                    }
                    StreamSignal::Quota { window, used: share, resets_at } => {
                        let reading = UsageWindow { provider: req.provider, window, group: None, used: share, resets_at };
                        let _ = budget::record(home, &[reading], Utc::now());
                    }
                    StreamSignal::Final { ok, text } => last = Some((ok, text)),
                    StreamSignal::Tool { name, summary } => {
                        match name.to_ascii_lowercase().as_str() {
                            "read" | "read_file" => counts[0] += 1,
                            "glob" | "grep" | "search" | "web_search" => counts[1] += 1,
                            "edit" | "write" | "multiedit" | "edit_file" | "write_file" | "apply_patch" => counts[2] += 1,
                            "bash" | "shell" | "command" | "execute_command" => counts[3] += 1,
                            _ => {}
                        }
                        progress.push(json!({ "name": name, "summary": summary }));
                        if progress.len() > 3 { progress.remove(0); }
                        let shown = partial_prose(&partial);
                        let state = json!({ "steps": progress, "counts": { "reads": counts[0], "searches": counts[1], "edits": counts[2], "commands": counts[3] }, "partial": shown });
                        if let Ok(text) = serde_json::to_string(&state) { let _ = fs::write(&progress_path, text); }
                    }
                }
            }
            Ok(())
        };
        let mut protocol_error = None;
        if app_server {
            match server_stdin.take() {
                None => fatal = Some("Codex app-server 입력을 열지 못했습니다".into()),
                Some(stdin) => {
                    let stdin = BufWriter::new(stdin);
                    let on_event = |event| {
                        if fatal.is_none() {
                            provider_finished |= matches!(event, providers::codex_app_server::Event::Finished { .. });
                            finished |= matches!(event, providers::codex_app_server::Event::Finished { ok: true, .. });
                            if let Some(line) = providers::codex_app_server::stream_line(event) {
                                fatal = process_line(&line).err();
                            }
                        }
                    };
                    #[cfg(windows)]
                    let result = if result_tools {
                        providers::codex_app_server::run_with_card_results(&turn, BufReader::new(stdout), stdin, on_event, |request| {
                            let guard = status_file.as_ref().ok_or("현재 실행 결과가 없습니다")?;
                            let result = if request.run_id == "*" && request.card_id == "*" && request.line.is_none() {
                                stream_status::list_results(guard, request.offset.unwrap_or(0))
                            } else {
                                stream_status::read_card_result(guard, &request.run_id, &request.card_id, request.line, request.offset.unwrap_or(0))
                            };
                            result
                                .map(|value| value.to_string()).map_err(|error| error.to_string())
                        })
                    } else {
                        providers::codex_app_server::run(&turn, BufReader::new(stdout), stdin, on_event)
                    };
                    #[cfg(not(windows))]
                    let result = providers::codex_app_server::run(&turn, BufReader::new(stdout), stdin, on_event);
                    protocol_error = result.err();
                }
            }
        } else {
            let mut lines = BufReader::new(stdout);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match lines.read_until(b'\n', &mut buf) {
                    Ok(0) => break,
                    Err(error) => { fatal = Some(format!("총괄 출력을 끝까지 읽지 못했습니다: {error}")); break; }
                    Ok(_) => {}
                }
                let text = String::from_utf8_lossy(&buf);
                if let Err(error) = process_line(text.trim_end_matches(['\r', '\n'])) {
                    fatal = Some(error);
                    break;
                }
            }
        }
        if let Some(error) = protocol_error { last = Some((false, error)); }
    }
    drop(server_stdin);
    let _ = fs::write(&progress_path, "[]");
    // The turn ends with an error below; the CLI must not outlive it.
    if fatal.is_some() { process.kill(); }
    // app-server is a long-lived process; do not let a stuck shutdown keep the turn open forever.
    // A turn it already finished keeps its result: only the process is ended.
    if app_server && fatal.is_none() {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if process.poll().is_some() || process.unknown() || process.cancelled() { break; }
            if Instant::now() >= deadline {
                process.kill();
                if !finished {
                    last = Some((false, "Codex app-server가 종료되지 않았습니다. 대화 상태를 확인하세요.".into()));
                }
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
    }
    let mut grace = None;
    let status = loop {
        let status = process.poll();
        let pipes_done = stdout_worker.as_ref().is_none_or(|h| h.is_finished()) && stderr.as_ref().is_none_or(|h| h.is_finished())
            && stdin_worker.as_ref().is_none_or(|h| h.is_finished());
        if status.is_some() && pipes_done { break status; }
        if process.unknown() { process.kill(); break None; }
        if process.cancelled() && Instant::now() >= *grace.get_or_insert_with(|| Instant::now() + Duration::from_secs(1)) {
            break status;
        }
        thread::sleep(Duration::from_millis(25));
    };
    let incomplete = status.is_none() || stdout_worker.as_ref().is_some_and(|h| !h.is_finished()) || stderr.as_ref().is_some_and(|h| !h.is_finished())
        || stdin_worker.as_ref().is_some_and(|h| !h.is_finished());
    let stop_asked = slot.stop_asked();
    let (reason, reap) = process.finish();
    let stderr_tail = stderr_tail.lock().unwrap().clone();
    let mut unknown_saved = true;
    // Without a job, closed pipes cannot prove that detached descendants have exited.
    let unresolved = incomplete || (!contained && reason != turn_process::End::Complete);
    if unresolved {
        slot.marker.unresolved = Some(true);
        unknown_saved = slot.write().is_ok();
        slot.keep_marker = true;
    }
    if incomplete {
        // Keep the turn locked until the process and inherited pipes actually finish.
        thread::spawn(move || {
            reap.wait();
            if let Some(worker) = stdout_worker { let _ = worker.join(); }
            if let Some(worker) = stderr { let _ = worker.join(); }
            if let Some(worker) = stdin_worker { let _ = worker.join(); }
            if !unknown_saved { loop { thread::park(); } }
            drop(slot);
        });
    } else if unresolved && !unknown_saved {
        thread::spawn(move || { let _slot = slot; loop { thread::park(); } });
    }
    if used {
        emit(LeadEvent::Usage { at: now_iso(), provider: req.provider, input, cached, output, complete: Some(usage_complete && (!app_server || provider_finished) && !incomplete) })?;
    }
    if let Some(error) = raw_issue {
        emit(LeadEvent::Error { at: now_iso(), text: format!("총괄 원시 출력 기록이 불완전합니다: {error}") })?;
    }
    if incomplete {
        let text = match reason {
            turn_process::End::Timeout => "총괄이 제한 시간을 넘겼고 종료 상태가 불명입니다. 프로세스와 출력을 확인하세요.",
            turn_process::End::Stop => "총괄 중지를 요청했지만 종료 상태가 불명입니다. 프로세스와 출력을 확인하세요.",
            _ => "총괄의 종료 상태가 불명입니다. 프로세스와 출력을 확인하세요.",
        };
        let mut text = if unknown_saved { text.to_string() } else { format!("{text} 불명 상태를 저장하지 못했습니다.") };
        if !stderr_tail.trim().is_empty() { text.push_str(&format!("\n최근 오류 출력: {}", stderr_tail.trim())); }
        emit(LeadEvent::Error { at: now_iso(), text })?;
        return Ok(added);
    }
    let timed_out = reason == turn_process::End::Timeout;
    let exit = status.and_then(proc::exit_code);
    if let Some(error) = fatal {
        return Err(if unresolved { format!("{error} 총괄 자손의 종료 상태도 불명입니다.") } else { error });
    }
    // The CLI got this turn's message: its session has seen the thread through it.
    let session_seen = used_session.is_some();
    if let Some(session_id) = used_session.filter(|_| worker.is_none()) {
        emit(LeadEvent::Seen { at: now_iso(), provider: req.provider, session_id, through: before.len() })?;
        if !pending.is_empty() && notes::change(home, &thread_id, notes::Change::Sent { notes: pending.clone() }).is_err() {
            emit(LeadEvent::Error { at: now_iso(), text: "주석은 전달했지만 전달 상태를 저장하지 못했습니다. 다시 보내기 전에 주석을 확인하세요".into() })?;
        }
    }
    if stop_asked || reason == turn_process::End::Stop {
        let text = if unknown_saved { "총괄 중지를 요청했지만 자손 프로세스 종료 여부는 불명입니다." }
            else { "총괄 중지를 요청했지만 자손 종료 여부가 불명이며 이 상태를 저장하지 못했습니다." };
        emit(if contained { stopped() } else { LeadEvent::Error { at: now_iso(), text: text.into() } })?;
        return Ok(added);
    }
    if timed_out {
        let minutes = limit.as_secs().div_ceil(60);
        let text = if contained { format!("총괄이 제한 시간({minutes}분) 안에 답하지 않아 멈췄습니다. 요청을 나눠 다시 보내세요") }
            else if unknown_saved { format!("총괄이 제한 시간({minutes}분)을 넘겨 종료를 요청했지만 자손 프로세스 종료 여부는 불명입니다.") }
            else { format!("총괄이 제한 시간({minutes}분)을 넘겼고 자손 종료 여부가 불명이며 이 상태를 저장하지 못했습니다.") };
        emit(LeadEvent::Error { at: now_iso(), text })?;
        return Ok(added);
    }
    if unresolved {
        let text = if unknown_saved { "총괄 자손의 종료 상태가 불명입니다." } else { "총괄 자손의 종료 상태가 불명이며 이 상태를 저장하지 못했습니다." };
        emit(LeadEvent::Warning { at: now_iso(), text: text.into() })?;
    }
    // A turn app-server finished needs no clean exit: a slow shutdown was ended above.
    let ok = last.as_ref().is_some_and(|(ok, _)| *ok) && (exit == Some(0) || finished);
    let answer = last.map(|(_, text)| text).filter(|t| !t.trim().is_empty()).unwrap_or(last_message);
    if !ok {
        let why = if limited {
            format!("{} 사용량 한도에 걸렸습니다. 다른 에이전트를 고르거나 한도가 풀린 뒤 다시 보내세요", req.provider)
        } else {
            [answer.trim(), stderr_tail.trim()].into_iter().find(|t| !t.is_empty()).map(str::to_string).unwrap_or_else(|| format!("종료 코드 {exit:?}"))
        };
        emit(LeadEvent::Error { at: now_iso(), text: format!("총괄이 답하지 못했습니다: {why}") })?;
        return Ok(added);
    }

    if let Some(w) = &worker {
        if answer.trim().is_empty() { emit(LeadEvent::Error { at: now_iso(), text: "카드 메시지에 대한 총괄 답변이 비어 있습니다".into() })?; return Ok(added); }
        emit(LeadEvent::Reply { at: now_iso(), provider: req.provider, text: answer.clone(), reported_files: Vec::new() })?;
        let current = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(leads_dir(home).join("current.lock")).map_err(|e| e.to_string())?;
        current.lock().map_err(|e| e.to_string())?;
        if store::stop_requested(home, &w.speaker.run_id) || current_thread(home, req.project).as_deref() != Some(&thread_id) {
            emit(LeadEvent::Error { at: now_iso(), text: "새 대화로 전환되어 카드에 답을 보내지 않았습니다".into() })?;
            return Ok(added);
        }
        if w.message.kind == "question" {
            let key = crate::run::result::digest(&format!("{}\0{}", w.speaker.run_id, w.message.id));
            let text = if js_len(&answer) > LIMITS.max_message_chars {
                let violations = crate::policy::check_message(&answer, &crate::policy::Parties { from: "lead", to: &w.message.from, known: &w.speaker.cards });
                let supports = store::read_plan(home, &w.speaker.run_id).ok().and_then(|v| serde_json::from_value::<crate::run::plan::Plan>(v).ok())
                    .and_then(|plan| plan.cards.into_iter().find(|card| card.id == w.message.from))
                    .is_some_and(|card| providers::get(card.provider).searches_context());
                if !supports || violations.iter().any(|v| v.rule != "msg-size") {
                    emit(LeadEvent::Error { at: now_iso(), text: "긴 답변의 원문 전달을 지원하지 않거나 정책 검사에 걸렸습니다. 대화에서 원문을 확인하세요".into() })?;
                    return Ok(added);
                }
                match crate::run::result::store_lead(home, &w.speaker.run_id, &answer) {
                    Ok(hash) => crate::run::result::lead_reference(&w.message.id, &hash),
                    Err(error) => { emit(LeadEvent::Error { at: now_iso(), text: format!("긴 답변을 저장하지 못했습니다: {error}") })?; return Ok(added); }
                }
            } else { answer.clone() };
            if team::send_reply(home, w.speaker, &w.message.from, &text, false, Some(&key), Some(&w.message.id)).is_err() {
                emit(LeadEvent::Error { at: now_iso(), text: "카드에 답을 보내지 못했습니다. 답변과 메시지 상태를 확인하세요".into() })?;
                return Ok(added);
            }
        }
        team::transition(home, w.speaker, &w.message.id, "acknowledged").map_err(|e| e.to_string())?;
        return Ok(added);
    }

    if !session_seen && !pending.is_empty() && notes::change(home, &thread_id, notes::Change::Sent { notes: pending }).is_err() {
        emit(LeadEvent::Error { at: now_iso(), text: "주석은 전달했지만 전달 상태를 저장하지 못했습니다. 다시 보내기 전에 주석을 확인하세요".into() })?;
    }

    let (prose, plan) = prompt::split_plan(&answer);
    let (prose, design) = prompt::split_design(&prose);
    let (prose, followups) = prompt::split_followups(&prose);
    let prose = if migrate {
        format!("결과 읽기 도구를 사용하려고 Codex 세션을 새로 시작했습니다. 이전 대화는 최근 일부만 전달되었으며 캐시가 초기화될 수 있습니다.\n\n{prose}")
    } else { prose };
    if !prose.is_empty() {
        emit(LeadEvent::Reply { at: now_iso(), provider: req.provider, text: prose, reported_files: reported_files.into_iter().collect() })?;
    }
    match design {
        Ok(Some(design)) => emit(LeadEvent::Design { at: now_iso(), design })?,
        Ok(None) => {},
        Err(message) => {
            emit(LeadEvent::Error { at: now_iso(), text: message })?;
            return Ok(added);
        }
    }
    match followups {
        Ok(None) => {},
        Err(message) => { emit(LeadEvent::Error { at: now_iso(), text: message })?; return Ok(added); }
        Ok(Some(items)) => {
            if unresolved { emit(LeadEvent::Error { at: now_iso(), text: "총괄 종료 상태가 불명이라 후속 지시를 보내지 않았습니다".into() })?; return Ok(added); }
            let Some(start) = start else { emit(LeadEvent::Error { at: now_iso(), text: "이 실행 환경에서는 후속 지시를 자동 전달할 수 없습니다".into() })?; return Ok(added); };
            if !matches!(plan, Ok(None)) { emit(LeadEvent::Error { at: now_iso(), text: "새 설계와 후속 지시가 함께 있어 실행하지 않았습니다".into() })?; return Ok(added); }
            // "새 설계" changes the current thread under current.lock: hold it while the thread is
            // checked and the follow-ups start, so they never go out for a thread just replaced.
            // Same order as `dispatch` (current.lock, then dispatch.lock). This turn already holds
            // turn.lock; that is safe because no current.lock holder waits on turn.lock (`dispatch`
            // only tries it, `thread_for` never takes it).
            let current = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(leads_dir(home).join("current.lock")).map_err(|e| e.to_string())?;
            current.lock().map_err(|e| e.to_string())?;
            let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(thread_dir(home, &thread_id).join("dispatch.lock")).map_err(|e| e.to_string())?;
            lock.lock().map_err(|e| e.to_string())?;
            let stopped = fs::read_to_string(thread_dir(home, &thread_id).join("stop-all-at")).ok().and_then(|at| chrono::DateTime::parse_from_rfc3339(&at).ok()).is_some_and(|at| at.with_timezone(&Utc) >= turn_started);
            if stopped || current_thread(home, req.project).as_deref() != Some(&thread_id) {
                emit(LeadEvent::Error { at: now_iso(), text: "중지되었거나 새 설계로 전환되어 후속 지시를 보내지 않았습니다".into() })?;
                return Ok(added);
            }
            let allowed: BTreeSet<_> = before.iter().filter_map(|e| match e { LeadEvent::Dispatch { run_id, .. } | LeadEvent::Followup { run_id, .. } => Some(run_id.as_str()), _ => None }).collect();
            let mut seen = BTreeSet::new();
            if items.iter().any(|item| !allowed.contains(item.run.as_str()) || !seen.insert((&item.run, &item.card))) {
                emit(LeadEvent::Error { at: now_iso(), text: "이 대화에서 분배한 서로 다른 작업만 이어갈 수 있습니다".into() })?;
                return Ok(added);
            }
            for item in items {
                match crate::run::followup::start(home, &item.run, &item.card, &item.task, start) {
                    Ok(run_id) => emit(LeadEvent::Followup { at: now_iso(), source_run_id: item.run, card_id: item.card, run_id })?,
                    Err(error) => emit(LeadEvent::Error { at: now_iso(), text: format!("{} 후속 지시를 보내지 못했습니다: {error}", item.card) })?,
                }
            }
        }
    }
    match plan {
        Ok(None) => {}
        Ok(Some(plan)) => {
            let plans = before.iter().filter(|e| matches!(e, LeadEvent::Plan { .. })).count();
            let (plan, violations) = check_lead_plan(home, req.project, plan, Profile::of_clean(req.clean.is_some()));
            emit(LeadEvent::Plan { at: now_iso(), plan_id: format!("{thread_id}-p{}", plans + 1), plan, ok: violations.is_empty(), violations })?;
        }
        Err(message) => emit(LeadEvent::Error { at: now_iso(), text: message })?,
    }
    Ok(added)
}

/// A plan from the lead with its project forced to the thread's, and what the policy says
/// of it now (`o dispatch`'s check: shape, providers, caps, cooldowns, today's budget, and
/// the profile the lead ran on, which the app dispatches the plan with).
pub fn check_lead_plan(home: &Path, project: &str, mut plan: Value, profile: Profile) -> (Value, Vec<Violation>) {
    if let Some(object) = plan.as_object_mut() {
        object.insert("project".into(), Value::String(project.to_string()));
    }
    let violations = match policy_context(home, Utc::now()) {
        Ok(ctx) => check_plan_for(&plan, &ctx, profile).violations,
        Err(error) => vec![Violation { rule: "policy-context".into(), message: error.to_string(), card_id: None }],
    };
    (plan, violations)
}

/// `<threadId>-p<n>` → the thread id.
fn plan_thread(plan_id: &str) -> Option<&str> {
    let (thread, n) = plan_id.rsplit_once("-p")?;
    (is_run_id(thread) && !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())).then_some(thread)
}

pub enum DispatchError {
    /// The policy refused the plan; nothing ran.
    Blocked(Vec<Violation>),
    Other(String),
}

impl From<String> for DispatchError {
    fn from(message: String) -> Self {
        DispatchError::Other(message)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dispatched {
    pub plan_id: String,
    pub run_id: String,
    pub events: String,
    pub cards: Vec<String>,
    pub profile: Profile,
    pub clean: bool,
    pub yolo: bool,
}

/// Dispatches a stored plan, or explicitly retries a failed run: checks it again like `o dispatch`, makes the run and
/// hands its id to `start` (which launches `o _run <runId>` in the background).
/// `mode` is the profile and context choice (ADR-0013), saved with the run like `o dispatch` does.
pub fn dispatch(home: &Path, plan_id: &str, mode: RunMode, start: &dyn Fn(&str) -> io::Result<()>) -> Result<Dispatched, DispatchError> {
    let _locks = lock_latest(home, plan_id)?;
    dispatch_locked(home, plan_id, mode, start, true)
}

/// The locks every dispatch (by hand or automatic) holds while it checks and starts a plan.
struct DispatchLocks {
    _current: fs::File,
    _turn: fs::File,
    _dispatch: fs::File,
}

/// Takes the dispatch locks in their one order (the thread list, the thread's turn, its
/// dispatch) and checks that the plan's thread is still the project's current one, that
/// no turn is answering, and that the plan is still the thread's latest word.
fn lock_latest(home: &Path, plan_id: &str) -> Result<DispatchLocks, String> {
    let thread_id = plan_thread(plan_id).ok_or_else(|| format!("계획 id 형식이 아닙니다: {plan_id}"))?;
    let current = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(leads_dir(home).join("current.lock")).map_err(|e| e.to_string())?;
    current.lock().map_err(|e| e.to_string())?;
    let turn = turn_lock(home, thread_id).map_err(|e| e.to_string())?;
    // `show` holds the lock for an instant when it looks; only a turn holds it longer.
    let deadline = Instant::now() + Duration::from_millis(300);
    loop {
        match turn.try_lock() {
            Ok(()) => break,
            Err(fs::TryLockError::WouldBlock) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Err(_) => return Err("총괄이 답하는 중입니다. 새 설계를 확인한 뒤 분배하세요".to_string()),
        }
    }
    if read_marker(home, thread_id).is_some_and(|marker| marker.unresolved == Some(true)) {
        return Err("이전 총괄의 종료 상태가 불명입니다. 프로세스와 출력을 확인하세요".into());
    }
    let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(thread_dir(home, thread_id).join("dispatch.lock")).map_err(|e| e.to_string())?;
    lock.lock().map_err(|e| e.to_string())?;
    let events = read_thread(home, thread_id);
    let project = thread_project(&events).ok_or_else(|| "총괄 대화를 찾지 못했습니다".to_string())?;
    let latest = events.iter().rev().find_map(|event| match event {
        LeadEvent::Plan { plan_id, .. } => Some(Some(plan_id.as_str())),
        LeadEvent::User { .. } => Some(None),
        _ => None,
    }).flatten();
    if current_thread(home, &project).as_deref() != Some(thread_id) || latest != Some(plan_id) {
        return Err("설계가 변경되었습니다. 최신 설계를 확인한 뒤 분배하세요".to_string());
    }
    Ok(DispatchLocks { _current: current, _turn: turn, _dispatch: lock })
}

fn dispatch_locked(home: &Path, plan_id: &str, mode: RunMode, start: &dyn Fn(&str) -> io::Result<()>, retry: bool) -> Result<Dispatched, DispatchError> {
    let thread_id = plan_thread(plan_id).ok_or_else(|| format!("계획 id 형식이 아닙니다: {plan_id}"))?;
    let events = read_thread(home, thread_id);
    let project = thread_project(&events).ok_or_else(|| format!("총괄 대화가 없습니다: {thread_id}"))?;
    let plan = events
        .iter()
        .find_map(|e| match e {
            LeadEvent::Plan { plan_id: id, plan, .. } if id == plan_id => Some(plan.clone()),
            _ => None,
        })
        .ok_or_else(|| format!("이 대화에 그런 계획이 없습니다: {plan_id}"))?;
    let previous_run = events.iter().rev().find_map(|e| match e {
        LeadEvent::Dispatch { plan_id: id, run_id, .. } if id == plan_id => Some(run_id.clone()),
        _ => None,
    });
    if let Some(run) = &previous_run {
        let failed = is_run_id(run) && snapshot(&read_events(home, run)).is_some_and(|view| view.status == RunStatus::Failed);
        if !retry || !failed || store::run_is_live(home, run) {
            return Err(format!("이미 실행한 계획입니다 (실행 {run})").into());
        }
    }

    let now = Utc::now();
    let mut plan = plan;
    if let Some(object) = plan.as_object_mut() {
        object.insert("project".into(), Value::String(project.clone()));
    }
    let ctx = policy_context(home, now).map_err(|e| e.to_string())?;
    let checked = check_plan_for(&plan, &ctx, mode.profile);
    let Some(plan) = checked.plan else {
        let _ = record_blocked(home, now, plan.get("title"), &checked.violations);
        return Err(DispatchError::Blocked(checked.violations));
    };

    crate::run::workspaces::preflight(&plan.project).map_err(|e| format!("작업 폴더를 분리할 수 없습니다: {e}"))?;

    // The marker prevents duplicate dispatch, including when a prior start did not leave an event.
    let marks = thread_dir(home, thread_id).join("dispatched");
    fs::create_dir_all(&marks).map_err(|e| e.to_string())?;
    let mark = marks.join(plan_id);
    let retry_mark = marks.join(format!("{plan_id}.retry"));
    if previous_run.is_some() && retry_mark.exists() {
        let recorded = fs::read_to_string(&retry_mark).map_err(|e| e.to_string())?;
        if previous_run.as_deref() != Some(recorded.as_str()) {
            return Err("이전 재실행의 시작 여부를 확인할 수 없어 중복 실행을 막았습니다".to_string().into());
        }
        fs::remove_file(&retry_mark).map_err(|e| e.to_string())?;
    }
    if previous_run.is_none() {
        match OpenOptions::new().write(true).create_new(true).open(&mark) {
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::AlreadyExists => return Err("이미 실행한 계획입니다".to_string().into()),
            Err(e) => return Err(e.to_string().into()),
        }
    }
    let run_id = new_run_id(now);
    if previous_run.is_some() {
        let mut marker = OpenOptions::new().write(true).create_new(true).open(&retry_mark).map_err(|e| e.to_string())?;
        marker.write_all(run_id.as_bytes()).and_then(|()| marker.sync_all()).map_err(|e| e.to_string())?;
    }
    let started = store::create_run(home, &run_id, &plan)
        .and_then(|()| store::mark_mode(home, &run_id, &mode))
        .and_then(|()| fs::write(run_dir(home, &run_id).join("isolated-worktrees"), ""))
        .and_then(|()| start(&run_id));
    if let Err(error) = started {
        // Nothing runs: the plan may be dispatched again.
        if previous_run.is_none() { let _ = fs::remove_file(&mark); } else { let _ = fs::remove_file(&retry_mark); }
        return Err(format!("실행기를 시작하지 못했습니다: {error}").into());
    }
    let dispatched = LeadEvent::Dispatch { at: now_iso(), plan_id: plan_id.to_string(), run_id: run_id.clone(), clean: mode.clean(), profile: Some(mode.profile), yolo: mode.yolo };
    append(home, thread_id, &dispatched).map_err(|e| e.to_string())?;
    if previous_run.is_some() { let _ = fs::remove_file(&retry_mark); }
    Ok(Dispatched {
        plan_id: plan_id.to_string(),
        events: run_dir(home, &run_id).join("events.jsonl").to_string_lossy().into_owned(),
        run_id,
        cards: plan.cards.iter().map(|c| c.id.clone()).collect(),
        profile: mode.profile,
        clean: mode.clean(),
        yolo: mode.yolo,
    })
}

/// Run a lead turn and, when allowed, hand its plan to the same checked dispatch path.
pub fn send_with_autorun(home: &Path, req: &SendRequest<'_>, spawner: &Spawner, start: &dyn Fn(&str) -> io::Result<()>) -> Result<Vec<LeadEvent>, String> {
    send_with_auto(home, req, spawner, start, None, false)
}

pub fn send_with_autorun_and_followups(home: &Path, req: &SendRequest<'_>, spawner: &Spawner, start: &dyn Fn(&str) -> io::Result<()>, resume: &dyn Fn(&str) -> io::Result<()>) -> Result<Vec<LeadEvent>, String> {
    send_with_auto(home, req, spawner, start, Some(resume), false)
}

/// A user-created schedule authorizes dispatch, but keeps the lead and cards in normal mode.
pub fn send_with_scheduled_autorun(home: &Path, req: &SendRequest<'_>, spawner: &Spawner, start: &dyn Fn(&str) -> io::Result<()>) -> Result<Vec<LeadEvent>, String> {
    if req.yolo { return Err("예약 요청은 일반 권한으로 실행해야 합니다".into()); }
    if req.provider == ProviderId::Agy { return Err("예약에서는 Antigravity를 실행할 수 없습니다".into()); }
    send_with_auto(home, req, spawner, start, None, true)
}

fn send_with_auto(home: &Path, req: &SendRequest<'_>, spawner: &Spawner, start: &dyn Fn(&str) -> io::Result<()>, resume: Option<&StartRun<'_>>, scheduled: bool) -> Result<Vec<LeadEvent>, String> {
    let turn_started = Utc::now();
    let mut added = send_within_followups(home, req, spawner, TURN_LIMIT, resume)?;
    let Some((plan_id, plan, ok)) = added.iter().find_map(|e| match e {
        LeadEvent::Plan { plan_id, plan, ok, .. } => Some((plan_id.clone(), plan.clone(), *ok)),
        _ => None,
    }) else { return Ok(added) };
    let thread = plan_thread(&plan_id).ok_or("계획 id 형식이 아닙니다")?;
    // The same locks and checks as a dispatch by hand: a newer thread, request or turn wins.
    // Nothing is written then, since another turn may own the thread.
    let _locks = match lock_latest(home, &plan_id) {
        Ok(locks) => locks,
        Err(why) => {
            added.push(LeadEvent::AutoRun { at: now_iso(), plan_id, hash: None, reason: Some(format!("자동 실행을 취소했습니다: {why}")) });
            return Ok(added);
        }
    };
    let events = read_thread(home, thread);
    let now = Utc::now();
    let checked = policy_context(home, now).map_err(|e| e.to_string()).map(|ctx| check_plan_for(&plan, &ctx, Profile::of_clean(req.clean.is_some())));
    let valid = checked.as_ref().ok().and_then(|result| result.plan.clone());
    let running: Vec<String> = events.iter().filter_map(|e| match e {
        LeadEvent::Dispatch { run_id, .. } | LeadEvent::Followup { run_id, .. } if store::run_is_live(home, run_id) => Some(run_id.clone()),
        _ => None,
    }).collect();
    // Only auto-runs that started count: a reservation followed by its plan's dispatch, not
    // by the note that the start failed (a missing `o`, a start error, the last policy check).
    let past: Vec<PastAutoRun> = events.iter().enumerate().filter_map(|(i, e)| match e {
        LeadEvent::AutoRun { at, plan_id, hash: Some(hash), .. } => {
            let started = events[i + 1..].iter().find_map(|next| match next {
                LeadEvent::Dispatch { plan_id: id, .. } if id == plan_id => Some(true),
                LeadEvent::AutoRun { plan_id: id, .. } if id == plan_id => Some(false),
                _ => None,
            }).unwrap_or(false);
            if !started { return None; }
            chrono::DateTime::parse_from_rfc3339(at).ok().map(|at| PastAutoRun { at: at.with_timezone(&Utc), hash: hash.clone() })
        }
        _ => None,
    }).collect();
    let hash = valid.as_ref().map(yolo::plan_hash);
    let stopped = fs::read_to_string(thread_dir(home, thread).join("stop-all-at")).ok()
        .and_then(|at| chrono::DateTime::parse_from_rfc3339(&at).ok()).is_some_and(|at| at.with_timezone(&Utc) >= turn_started);
    let reason = if stopped { Some("모두 중지 요청으로 자동 실행을 취소했습니다".to_string()) }
        else if !req.yolo && !scheduled { Some("YOLO 모드가 꺼져 있어 실행을 기다립니다".to_string()) }
        else if checked.as_ref().ok().is_some_and(|result| result.violations.iter().any(|v| v.rule == "provider-terms")) { Some(crate::policy::AGY_UNAVAILABLE.to_string()) }
        else if let (true, Some(valid_plan), Some(plan_hash)) = (ok, valid.as_ref(), hash.as_deref()) {
            let facts = AutoRunFacts { budget_on: budget::read_enabled_for_autorun(home), cards: valid_plan.cards.len(), running: &running, past: &past, hash: plan_hash, now };
            yolo::check_auto_run(&facts, yolo::read_limits(home)).err()
        } else { Some("정책 검사에 통과하지 못해 실행을 기다립니다".to_string()) };
    let run = reason.is_none();
    let event = LeadEvent::AutoRun { at: now_iso(), plan_id: plan_id.clone(), hash: if run { hash } else { None }, reason: reason.or_else(|| Some("자동 실행을 준비 중입니다".to_string())) };
    append(home, thread, &event).map_err(|e| e.to_string())?;
    added.push(event);
    if run {
        let mode = RunMode { yolo: req.yolo, ..RunMode::of(Profile::of_clean(req.clean.is_some())) };
        match dispatch_locked(home, &plan_id, mode, start, false) {
            Ok(_) => added.extend(read_thread(home, thread).into_iter().rev().find(|e| matches!(e, LeadEvent::Dispatch { plan_id: id, .. } if id == &plan_id))),
            Err(e) => {
                let why = match e {
                    DispatchError::Other(message) => message,
                    DispatchError::Blocked(violations) => violations.into_iter().map(|v| v.message).collect::<Vec<_>>().join(" / "),
                };
                let event = LeadEvent::AutoRun { at: now_iso(), plan_id: plan_id.clone(), hash: None, reason: Some(format!("자동 실행을 시작하지 못했습니다: {why}")) };
                append(home, thread, &event).map_err(|e| e.to_string())?;
                added.push(event);
            }
        }
    }
    Ok(added)
}

/// Stops the lead turn of the project's current thread (every live turn without a
/// project): a stop request, then the CLI's process tree. `false` when none was running.
pub fn stop(home: &Path, project: Option<&str>) -> bool {
    let threads: Vec<String> = match project {
        Some(project) => current_thread(home, project).into_iter().collect(),
        None => read_current(home).unwrap_or_default().into_values().filter(|id| is_run_id(id)).collect(),
    };
    let mut stopped = false;
    for thread_id in threads {
        if let Some(busy) = running(home, &thread_id) {
            // A request the turn's owner acts on: before its CLI starts it does not start it, after
            // it ends it. Nothing is killed from here, so the owner (the app, or `o lead send`)
            // is never ended and no pid is trusted. The marker may be a moment from being written.
            let mut marker = busy.marker;
            for _ in 0..20 {
                if marker.is_some() {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
                marker = read_marker(home, &thread_id);
            }
            let _ = fs::write(stop_path(home, &thread_id), marker.map(|m| m.token).unwrap_or_default());
            stopped = true;
        } else if let Some(stopped_orphan) = clear_dead_turn(home, &thread_id) {
            stopped |= stopped_orphan;
        }
    }
    stopped
}

/// Stop the lead and live runs started by this project's current thread.
pub fn stop_all(home: &Path, project: &str) -> Result<usize, String> {
    let Some(thread) = current_thread(home, project) else { return Ok(0) };
    let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(thread_dir(home, &thread).join("dispatch.lock")).map_err(|e| e.to_string())?;
    lock.lock().map_err(|e| e.to_string())?;
    fs::write(thread_dir(home, &thread).join("stop-all-at"), Utc::now().to_rfc3339()).map_err(|e| e.to_string())?;
    let mut count = usize::from(stop(home, Some(project)));
    let mut runs: BTreeSet<String> = read_thread(home, &thread).into_iter().filter_map(|event| match event {
        LeadEvent::Dispatch { run_id, .. } | LeadEvent::Followup { run_id, .. } if is_run_id(&run_id) => Some(run_id),
        _ => None,
    }).collect();
    let references: Vec<(String, String)> = fs::read_dir(home.join("runs")).into_iter().flatten().flatten().filter_map(|entry| {
        let id = entry.file_name().to_string_lossy().into_owned();
        if !is_run_id(&id) { return None; }
        let (source, _): (String, String) = serde_json::from_slice(&fs::read(entry.path().join("followup.json")).ok()?).ok()?;
        is_run_id(&source).then_some((id, source))
    }).collect();
    loop {
        let before = runs.len();
        for (id, source) in &references { if runs.contains(source) { runs.insert(id.clone()); } }
        if runs.len() == before { break; }
    }
    for run_id in runs {
        if store::run_is_live(home, &run_id) && crate::run::runner::stop_run(home, &run_id).map_err(|e| e.to_string())? { count += 1; }
    }
    Ok(count)
}

/// A marker whose owner is gone, cleared under the turn lock (so never a new turn's). Its
/// CLI, if the owner left it running (no kill-on-close job held it), is ended only when its
/// pid still names the process the turn started (same start time). `Some(ended)` when a
/// marker was there.
fn clear_dead_turn(home: &Path, thread_id: &str) -> Option<bool> {
    let lock = turn_lock(home, thread_id).ok()?;
    lock.try_lock().ok()?;
    let marker = read_marker(home, thread_id);
    let path = running_path(home, thread_id);
    if marker.is_none() && !path.exists() {
        return None;
    }
    if marker.as_ref().is_some_and(|marker| marker.unresolved == Some(true)) { return Some(false); }
    let mut ended = false;
    if let Some(orphan) = marker.and_then(|m| m.child) {
        let started = proc::started_at(orphan.pid);
        if started.is_some() && started == orphan.started {
            proc::kill_tree(orphan.pid);
            ended = true;
        }
    }
    let _ = fs::remove_file(path);
    Some(ended)
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadView {
    pub thread_id: String,
    pub project: String,
    /// A turn is running: its provider, or `"starting"` before its marker is written.
    pub busy: Option<String>,
    /// Recent tools from this turn; never part of conversation history.
    pub progress: Vec<Value>,
    pub progress_counts: Value,
    /// Incomplete prose for display during a turn; never conversation history.
    pub partial: String,
    pub notes: Vec<notes::Note>,
    pub notes_error: Option<String>,
    /// Every event as written (screens ignore kinds they do not know).
    pub events: Vec<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeadProject {
    pub project: String,
    pub thread_id: String,
}

/// What `o lead show --json` prints: the project's current thread (or none), and every
/// project that has a lead thread, newest thread first.
pub fn show(home: &Path, project: Option<&str>) -> Value {
    let current = read_current(home).unwrap_or_default();
    let mut projects: Vec<LeadProject> = current
        .values()
        .filter(|id| is_run_id(id))
        .filter_map(|id| Some(LeadProject { project: thread_project(&read_thread(home, id))?, thread_id: id.clone() }))
        .collect();
    projects.sort_by(|a, b| b.thread_id.cmp(&a.thread_id));
    let thread_id = match project {
        Some(project) => current_thread(home, project),
        None => projects.first().map(|p| p.thread_id.clone()),
    };
    let thread = thread_id.map(|id| {
        let (notes, notes_error) = match notes::read(home, &id) { Ok(notes) => (notes, None), Err(error) => (Vec::new(), Some(error.to_string())) };
        let busy = running(home, &id).map(|b| b.marker.map_or_else(|| STARTING.to_string(), |m| m.provider.as_str().to_string()));
        let state: Value = busy.as_ref().and_then(|_| fs::read(thread_dir(home, &id).join("progress.json")).ok())
            .and_then(|data| serde_json::from_slice(&data).ok()).unwrap_or_default();
        let progress = state.get("steps").or_else(|| state.is_array().then_some(&state)).and_then(Value::as_array).cloned().unwrap_or_default();
        let progress_counts = state.get("counts").cloned().unwrap_or(Value::Null);
        let partial = state.get("partial").and_then(Value::as_str).unwrap_or_default().to_string();
        ThreadView {
            project: thread_project(&read_thread(home, &id)).unwrap_or_default(),
            busy,
            progress,
            progress_counts,
            partial,
            notes,
            notes_error,
            events: event_values(home, &id),
            thread_id: id,
        }
    });
    json!({ "thread": thread, "projects": projects })
}

fn thread_ids(home: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(leads_dir(home)) else { return Vec::new() };
    entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|id| is_run_id(id)).collect()
}

/// All active turns, including a thread replaced by a newer one for the same project.
pub fn active_count(home: &Path) -> io::Result<usize> {
    let entries = match fs::read_dir(leads_dir(home)) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut count = 0;
    for entry in entries {
        let id = entry?.file_name().to_string_lossy().into_owned();
        if !is_run_id(&id) { continue; }
        let lock = turn_lock(home, &id)?;
        match lock.try_lock() {
            Ok(()) => {},
            Err(fs::TryLockError::WouldBlock) => count += 1,
            Err(fs::TryLockError::Error(error)) => return Err(error),
        }
    }
    Ok(count)
}

fn sessions_path(home: &Path, thread_id: &str) -> PathBuf {
    thread_dir(home, thread_id).join("sessions.txt")
}

/// A thread's CLI session ids, one per line in `sessions.txt`. A thread from before that
/// index gets it from its events, once (`create_new`: a turn that made it first keeps its own).
fn thread_sessions(home: &Path, thread_id: &str) -> Vec<String> {
    let path = sessions_path(home, thread_id);
    if let Ok(text) = fs::read_to_string(&path) {
        return text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect();
    }
    let ids: Vec<String> = read_thread(home, thread_id)
        .into_iter()
        .filter_map(|e| match e {
            LeadEvent::Session { session_id, .. } => Some(session_id),
            _ => None,
        })
        .collect();
    if let Ok(mut file) = OpenOptions::new().write(true).create_new(true).open(&path) {
        let _ = file.write_all(ids.iter().map(|id| format!("{id}\n")).collect::<String>().as_bytes());
    }
    ids
}

/// Adds a session id to its thread's index (made from the events first if the thread is older).
fn index_session(home: &Path, thread_id: &str, session_id: &str) {
    thread_sessions(home, thread_id);
    if let Ok(mut file) = OpenOptions::new().append(true).open(sessions_path(home, thread_id)) {
        let _ = file.write_all(format!("{session_id}\n").as_bytes());
    }
}

/// Every CLI session a lead turn used, so `o runs --sessions` does not list the lead as separate
/// work. Read from each thread's small index: this runs on every 2-second runs poll.
pub fn session_ids(home: &Path) -> Vec<String> {
    thread_ids(home).iter().flat_map(|id| thread_sessions(home, id)).collect()
}

/// Tokens lead turns spent (`o tokens`), for one project or all, since an ISO time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct LeadTokens {
    pub turns: u64,
    /// User requests in the selected period, including requests still running or not started.
    pub requested: u64,
    /// Requests without a subsequent usage event before the next request in this thread.
    pub unreported: u64,
    /// Usage events with missing fields or old records whose completeness is unknown.
    pub incomplete: u64,
    pub input: u64,
    pub cached: u64,
    pub output: u64,
    pub total: u64,
    #[serde(skip)]
    pub complete_tokens: crate::context::tokens::Tokens,
}

pub fn tokens(home: &Path, project: Option<&str>, since: Option<&str>) -> LeadTokens {
    let mut sum = LeadTokens::default();
    for id in thread_ids(home) {
        let events = read_thread(home, &id);
        if let Some(project) = project {
            if thread_project(&events).is_none_or(|p| project_key(&p) != project_key(project)) {
                continue;
            }
        }
        let mut selected = false;
        let mut reported = false;
        for event in events {
            match event {
                LeadEvent::User { at, .. } | LeadEvent::Worker { at, .. } => {
                    if selected && !reported { sum.unreported += 1; }
                    selected = since.is_none_or(|s| at.as_str() >= s);
                    reported = false;
                    if selected { sum.requested += 1; }
                }
                LeadEvent::Usage { at, input, cached, output, complete, .. } => {
                    if selected { reported = true; }
                    if since.is_some_and(|s| at.as_str() < s) { continue; }
                    let complete = complete == Some(true) && [input, cached, output].iter().all(|n| n.is_finite() && *n >= 0.0);
                    let (i, c, o) = (input.max(0.0) as u64, cached.max(0.0) as u64, output.max(0.0) as u64);
                    sum.turns += 1;
                    if !complete { sum.incomplete += 1; }
                    sum.input += i;
                    sum.cached += c;
                    sum.output += o;
                    sum.total += i + c + o;
                    if complete {
                        sum.complete_tokens.input += i;
                        sum.complete_tokens.cached += c;
                        sum.complete_tokens.output += o;
                        sum.complete_tokens.total += i + c + o;
                    }
                }
                _ => {}
            }
        }
        if selected && !reported { sum.unreported += 1; }
    }
    sum
}
