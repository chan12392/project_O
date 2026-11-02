//! The lead chat (ADR-0008): a person writes to a project's lead, an official CLI
//! reads the project (read-only) and proposes a work-card plan, and only the
//! person's "실행" makes O dispatch it, through the same policy as `o dispatch`.
//!
//! ```text
//! leads/current.json                 project (normalized) → its current thread id
//! leads/<threadId>/events.jsonl      the thread: lead.start · lead.user · lead.session · lead.reply
//!                                    · lead.plan · lead.dispatch · lead.usage · lead.error · lead.seen
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
pub mod prompt;

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc;
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
use crate::run::snapshot::snapshot;
use crate::run::yolo::{self, AutoRunFacts, PastAutoRun};
use crate::run::store::{self, is_run_id, new_run_id, policy_context, read_events, record_blocked, run_dir};
use crate::sys::js::{js_len, js_tail, now_iso};
use crate::sys::proc;
use crate::usage::{budget, UsageWindow};

pub use files::Attachment;

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
    },
    /// The lead's answer as Markdown, without its plan block.
    #[serde(rename = "lead.reply")]
    Reply { at: String, provider: ProviderId, text: String },
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
    #[serde(rename = "lead.autorun")]
    AutoRun { at: String, plan_id: String, hash: Option<String>, reason: Option<String> },
    /// Tokens one lead turn spent (`o tokens` counts them as planning overhead).
    #[serde(rename = "lead.usage")]
    Usage { at: String, provider: ProviderId, input: f64, cached: f64, output: f64 },
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
        // A stop file here is an older turn's: this one has no marker yet, so none was meant for it.
        let _ = fs::remove_file(stop_path(home, thread_id));
        let marker = Running { provider, owner: std::process::id(), token: new_run_id(Utc::now()), child: None };
        let slot = TurnSlot { _lock: lock, path: running_path(home, thread_id), stop: stop_path(home, thread_id), marker };
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
        // The marker and its stop before the lock: the next turn never finds them under its own lock.
        let _ = fs::remove_file(&self.path);
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

/// How each run the thread dispatched is doing, oldest first.
fn dispatched_runs(home: &Path, events: &[LeadEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            LeadEvent::Dispatch { run_id, .. } if is_run_id(run_id) => Some(match snapshot(&read_events(home, run_id)) {
                Some(view) => prompt::run_status(&view),
                None => format!("- 실행 {run_id}: 아직 기록 없음(시작 전이거나 시작하지 못함)"),
            }),
            _ => None,
        })
        .collect()
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
    let Some(next) = events.iter().skip(seen + 1).position(|e| matches!(e, LeadEvent::User { .. })) else { return Vec::new() };
    events[seen + 1 + next..]
        .iter()
        .filter_map(|e| match e {
            LeadEvent::User { text, .. } => Some(("사용자".to_string(), text.clone())),
            LeadEvent::Reply { text, provider, .. } => Some((format!("총괄({provider})"), text.clone())),
            _ => None,
        })
        .collect()
}

fn budget_lines(home: &Path) -> Vec<String> {
    budget::report_now(home).map(|r| r.providers.into_iter().map(|p| p.message).filter(|m| !m.is_empty()).collect()).unwrap_or_default()
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
    check_request(home, req)?;
    let thread_id = thread_for(home, req.project, req.fresh).map_err(|e| e.to_string())?;
    let mut slot = TurnSlot::take(home, &thread_id, req.provider)?;
    let before = read_thread(home, &thread_id);
    let attachments = req.attach.iter().map(|path| files::import(home, &thread_id, path)).collect::<Result<Vec<_>, _>>()?;

    let mut added = Vec::new();
    let mut emit = |event: LeadEvent| -> Result<(), String> {
        append(home, &thread_id, &event).map_err(|e| e.to_string())?;
        added.push(event);
        Ok(())
    };
    emit(LeadEvent::User {
        at: now_iso(),
        text: req.text.trim().to_string(),
        attachments: attachments.clone(),
        provider: req.provider,
        model: req.model.filter(|m| !m.is_empty()).map(str::to_string),
        effort: req.effort.filter(|e| !e.is_empty()).map(str::to_string),
        profile: Some(Profile::of_clean(req.clean.is_some())),
        yolo: req.yolo,
    })?;

    let session = before.iter().rev().find_map(|e| match e {
        LeadEvent::Session { provider, session_id, clean, .. } if *provider == req.provider && clean.is_none_or(|c| c == req.clean.is_some()) => {
            Some(session_id.clone())
        }
        _ => None,
    });
    let history: Vec<(String, String)> = before
        .iter()
        .filter_map(|e| match e {
            LeadEvent::User { text, .. } => Some(("사용자".to_string(), text.clone())),
            LeadEvent::Reply { text, .. } => Some(("총괄".to_string(), text.clone())),
            _ => None,
        })
        .collect();
    let missed = session.as_deref().map(|id| missed_turns(&before, req.provider, id)).unwrap_or_default();
    let runs = dispatched_runs(home, &before);
    let budget = budget_lines(home);
    let text = prompt::build(&prompt::PromptInput {
        project: req.project,
        text: req.text,
        attachments: &attachments,
        budget: &budget,
        runs: &runs,
        resumed: session.is_some(),
        history: &history,
        missed: &missed,
        yolo: req.yolo,
    });
    let files_dir = files::thread_files(home, &thread_id);
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
            let _ = clean::trust_preflight(root, req.provider, Path::new(req.project));
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
    let _tree = proc::contain(&child);
    slot.set_child(child.id());
    // The owner ends its own CLI when a stop comes, from this process or another: it holds the
    // child's handle, so the pid cannot have gone to another process. Checked at once, then often.
    // Past the time limit it ends it the same way, and says so (`true`).
    let (done, finished) = mpsc::channel::<()>();
    let watcher = {
        let (stop, token, pid) = (slot.stop.clone(), slot.marker.token.clone(), child.id());
        let deadline = Instant::now() + limit;
        thread::spawn(move || loop {
            if fs::read_to_string(&stop).is_ok_and(|t| t.is_empty() || t == token) {
                proc::kill_tree(pid);
                return false;
            }
            if Instant::now() >= deadline {
                proc::kill_tree(pid);
                return true;
            }
            match finished.recv_timeout(STOP_POLL) {
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                _ => return false,
            }
        })
    };
    if let Some(mut stdin) = child.stdin.take() {
        let text = spec.stdin.clone().unwrap_or_default();
        thread::spawn(move || {
            let _ = stdin.write_all(text.as_bytes());
        });
    }
    let stderr = child.stderr.take().map(|mut stderr| {
        thread::spawn(move || {
            let mut tail = String::new();
            let mut chunk = [0u8; 8192];
            while let Ok(n) = stderr.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                tail.push_str(&String::from_utf8_lossy(&chunk[..n]));
                tail = js_tail(&tail, 400).to_string();
            }
            tail
        })
    });

    let mut reader = StreamReader::new(req.provider);
    // The session this turn ran in: the resumed one, or the one the stream names.
    let mut used_session = session.clone();
    let (mut input, mut cached, mut output) = (0.0, 0.0, 0.0);
    let mut used = false;
    let mut last_message = String::new();
    let mut last: Option<(bool, String)> = None;
    let mut limited = false;
    if let Some(stdout) = child.stdout.take() {
        let mut lines = BufReader::new(stdout);
        let mut buf = Vec::new();
        let mut raw = OpenOptions::new().create(true).append(true).open(dir.join("turn.raw.jsonl")).ok();
        loop {
            buf.clear();
            match lines.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let text = String::from_utf8_lossy(&buf);
            let line = text.trim_end_matches(['\r', '\n']);
            if let Some(raw) = raw.as_mut() {
                let _ = raw.write_all(format!("{line}\n").as_bytes());
            }
            for signal in reader.read(line) {
                match signal {
                    StreamSignal::Session { session_id } => {
                        if session.as_deref() != Some(session_id.as_str()) {
                            emit(LeadEvent::Session { at: now_iso(), provider: req.provider, session_id: session_id.clone(), clean: Some(req.clean.is_some()) })?;
                            index_session(home, &thread_id, &session_id);
                        }
                        used_session = Some(session_id);
                    }
                    StreamSignal::Message { text } => last_message = text,
                    StreamSignal::Usage { input: i, cached: c, output: o } => {
                        (input, cached, output, used) = (input + i, cached + c, output + o, true);
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
                    StreamSignal::Tool { .. } => {}
                }
            }
        }
    }
    // The watcher is done before the child is reaped, so it never ends a pid reused since.
    drop(done);
    let timed_out = watcher.join().unwrap_or(false);
    let stderr_tail = stderr.and_then(|h| h.join().ok()).unwrap_or_default();
    let exit = child.wait().ok().and_then(proc::exit_code);
    // The CLI got this turn's message: its session has seen the thread through it.
    if let Some(session_id) = used_session {
        emit(LeadEvent::Seen { at: now_iso(), provider: req.provider, session_id, through: before.len() })?;
    }
    if used {
        emit(LeadEvent::Usage { at: now_iso(), provider: req.provider, input, cached, output })?;
    }

    if slot.stop_asked() {
        emit(stopped())?;
        return Ok(added);
    }
    if timed_out {
        let minutes = limit.as_secs().div_ceil(60);
        emit(LeadEvent::Error { at: now_iso(), text: format!("총괄이 제한 시간({minutes}분) 안에 답하지 않아 멈췄습니다. 요청을 나눠 다시 보내세요") })?;
        return Ok(added);
    }
    let ok = last.as_ref().is_some_and(|(ok, _)| *ok) && exit == Some(0);
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

    let (prose, plan) = prompt::split_plan(&answer);
    if !prose.is_empty() {
        emit(LeadEvent::Reply { at: now_iso(), provider: req.provider, text: prose })?;
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

/// Dispatches a stored plan once: checks it again like `o dispatch`, makes the run and
/// hands its id to `start` (which launches `o _run <runId>` in the background).
/// `mode` is the profile and context choice (ADR-0013), saved with the run like `o dispatch` does.
pub fn dispatch(home: &Path, plan_id: &str, mode: RunMode, start: &dyn Fn(&str) -> io::Result<()>) -> Result<Dispatched, DispatchError> {
    let thread_id = plan_thread(plan_id).ok_or_else(|| format!("계획 id 형식이 아닙니다: {plan_id}"))?;
    let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(thread_dir(home, thread_id).join("dispatch.lock")).map_err(|e| e.to_string())?;
    lock.lock().map_err(|e| e.to_string())?;
    dispatch_locked(home, plan_id, mode, start)
}

fn dispatch_locked(home: &Path, plan_id: &str, mode: RunMode, start: &dyn Fn(&str) -> io::Result<()>) -> Result<Dispatched, DispatchError> {
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
    if let Some(run) = events.iter().find_map(|e| match e {
        LeadEvent::Dispatch { plan_id: id, run_id, .. } if id == plan_id => Some(run_id.clone()),
        _ => None,
    }) {
        return Err(format!("이미 실행한 계획입니다 (실행 {run})").into());
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

    // Made once: a second dispatch of the same plan, even a concurrent one, stops here.
    let marks = thread_dir(home, thread_id).join("dispatched");
    fs::create_dir_all(&marks).map_err(|e| e.to_string())?;
    let mark = marks.join(plan_id);
    match OpenOptions::new().write(true).create_new(true).open(&mark) {
        Ok(_) => {}
        Err(e) if e.kind() == ErrorKind::AlreadyExists => return Err("이미 실행한 계획입니다".to_string().into()),
        Err(e) => return Err(e.to_string().into()),
    }
    let run_id = new_run_id(now);
    let started = store::create_run(home, &run_id, &plan)
        .and_then(|()| store::mark_mode(home, &run_id, &mode))
        .and_then(|()| start(&run_id));
    if let Err(error) = started {
        // Nothing runs: the plan may be dispatched again.
        let _ = fs::remove_file(&mark);
        return Err(format!("실행기를 시작하지 못했습니다: {error}").into());
    }
    let dispatched = LeadEvent::Dispatch { at: now_iso(), plan_id: plan_id.to_string(), run_id: run_id.clone(), clean: mode.clean(), profile: Some(mode.profile), yolo: mode.yolo };
    append(home, thread_id, &dispatched).map_err(|e| e.to_string())?;
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
    let turn_started = Utc::now();
    let mut added = send(home, req, spawner)?;
    let Some((plan_id, plan, ok)) = added.iter().find_map(|e| match e {
        LeadEvent::Plan { plan_id, plan, ok, .. } => Some((plan_id.clone(), plan.clone(), *ok)),
        _ => None,
    }) else { return Ok(added) };
    let thread = plan_thread(&plan_id).ok_or("계획 id 형식이 아닙니다")?;
    let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(thread_dir(home, thread).join("dispatch.lock")).map_err(|e| e.to_string())?;
    lock.lock().map_err(|e| e.to_string())?;
    let events = read_thread(home, thread);
    let now = Utc::now();
    let checked = policy_context(home, now).map_err(|e| e.to_string()).map(|ctx| check_plan_for(&plan, &ctx, Profile::of_clean(req.clean.is_some())));
    let valid = checked.ok().and_then(|result| result.plan);
    let running: Vec<String> = events.iter().filter_map(|e| match e {
        LeadEvent::Dispatch { run_id, .. } if store::run_is_live(home, run_id) => Some(run_id.clone()),
        _ => None,
    }).collect();
    let past: Vec<PastAutoRun> = events.iter().filter_map(|e| match e {
        LeadEvent::AutoRun { at, hash: Some(hash), .. } => chrono::DateTime::parse_from_rfc3339(at).ok().map(|at| PastAutoRun { at: at.with_timezone(&Utc), hash: hash.clone() }),
        _ => None,
    }).collect();
    let hash = valid.as_ref().map(yolo::plan_hash);
    let stopped = fs::read_to_string(thread_dir(home, thread).join("stop-all-at")).ok()
        .and_then(|at| chrono::DateTime::parse_from_rfc3339(&at).ok()).is_some_and(|at| at.with_timezone(&Utc) >= turn_started);
    let reason = if stopped { Some("모두 중지 요청으로 자동 실행을 취소했습니다".to_string()) }
        else if !req.yolo { Some("YOLO 모드가 꺼져 있어 실행을 기다립니다".to_string()) }
        else if let (true, Some(valid_plan), Some(plan_hash)) = (ok, valid.as_ref(), hash.as_deref()) {
            let facts = AutoRunFacts { budget_on: budget::read_enabled_for_autorun(home), cards: valid_plan.cards.len(), running: &running, past: &past, hash: plan_hash, now };
            yolo::check_auto_run(&facts, yolo::read_limits(home)).err()
        } else { Some("정책 검사에 통과하지 못해 실행을 기다립니다".to_string()) };
    let run = reason.is_none();
    let event = LeadEvent::AutoRun { at: now_iso(), plan_id: plan_id.clone(), hash: if run { hash } else { None }, reason: reason.or_else(|| Some("자동 실행을 준비 중입니다".to_string())) };
    append(home, thread, &event).map_err(|e| e.to_string())?;
    added.push(event);
    if run {
        let mode = RunMode { yolo: true, ..RunMode::of(Profile::of_clean(req.clean.is_some())) };
        match dispatch_locked(home, &plan_id, mode, start) {
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
    for event in read_thread(home, &thread) {
        if let LeadEvent::Dispatch { run_id, .. } = event {
            if store::run_is_live(home, &run_id) && crate::run::runner::stop_run(home, &run_id).map_err(|e| e.to_string())? { count += 1; }
        }
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
    let thread = thread_id.map(|id| ThreadView {
        project: thread_project(&read_thread(home, &id)).unwrap_or_default(),
        busy: running(home, &id).map(|b| b.marker.map_or_else(|| STARTING.to_string(), |m| m.provider.as_str().to_string())),
        events: event_values(home, &id),
        thread_id: id,
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
    pub input: u64,
    pub cached: u64,
    pub output: u64,
    pub total: u64,
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
        for event in events {
            if let LeadEvent::Usage { at, input, cached, output, .. } = event {
                if since.is_some_and(|s| at.as_str() < s) {
                    continue;
                }
                let (i, c, o) = (input.max(0.0) as u64, cached.max(0.0) as u64, output.max(0.0) as u64);
                sum.turns += 1;
                sum.input += i;
                sum.cached += c;
                sum.output += o;
                sum.total += i + c + o;
            }
        }
    }
    sum
}
