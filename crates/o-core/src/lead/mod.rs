//! The lead chat (ADR-0008): a person writes to a project's lead, an official CLI
//! reads the project (read-only) and proposes a work-card plan, and only the
//! person's "실행" makes O dispatch it, through the same policy as `o dispatch`.
//!
//! ```text
//! leads/current.json                 project (normalized) → its current thread id
//! leads/open.json                    project → open tabs and the one writer (ADR-0034)
//! leads/<threadId>/events.jsonl      the thread: lead.start · lead.user · lead.session · lead.reply
//!                                    · lead.plan · lead.dispatch · lead.usage · lead.warning · lead.error · lead.seen
//!                                    · lead.approval · lead.decision · lead.step
//! leads/<threadId>/turn.raw.jsonl    the CLIs' stdout, for reading back a turn
//! leads/<threadId>/sessions.txt      the CLI session ids it used, one per line (`o runs --sessions` reads these)
//! leads/<threadId>/turn.lock         held (OS lock) by the turn in progress, for as long as it runs
//! leads/<threadId>/running.json      that turn's marker ({ provider, owner, token, child }); stop: its stop request
//! leads/<threadId>/approvals/        requests waiting for the person and their answers (`approvals`)
//! leads/<threadId>/dispatched/<planId>   made once, when a plan is dispatched
//! <base>/project_O-lead/<threadId>/  attachments (`files`), outside O's state folder
//! ```
//!
//! Lead events live in their own file with their own type ([`LeadEvent`]); run
//! events (`run::events`) are untouched. The lead's answer is data: its plan is
//! checked like any plan before it is stored, and again before it runs.

pub mod approvals;
pub mod files;
pub mod log_page;
pub mod notes;
pub mod overall;
pub mod prompt;
pub mod recovery;
pub mod steps;
mod status_file;
mod tabs;
mod turn_process;
mod turn;
mod answer;
mod worker_reply;
mod event;
mod dispatch;
mod view;
#[cfg(windows)]
mod stream_status;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
#[cfg(not(any(windows, target_os = "macos")))]
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agents::clean;
use crate::context::normalize_project;
use crate::policy::{check_plan_in, check_project, is_model_id, Violation, LIMITS};
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

use turn::send_turn;
use view::{index_session, sessions_path, thread_ids};
use dispatch::{apply_review, plan_thread};
pub use event::LeadEvent;
#[cfg(test)]
use dispatch::lock_latest;
pub use dispatch::{check_lead_plan, dispatch, send_with_autorun, send_with_autorun_and_followups, send_with_scheduled_autorun, DispatchError, Dispatched};
pub use view::{active_count, session_ids, show, stop, stop_all, tokens, LeadProject, LeadTab, LeadTokens, ThreadView};
pub use approvals::decide;
pub use files::Attachment;
pub use tabs::{attachment_thread, close, is_open, open_threads, run_thread, select_thread, set_writer, MAX_OPEN};
type StartRun<'a> = dyn Fn(&str) -> io::Result<()> + 'a;

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
    select_thread(home, project, None, fresh).map_err(io::Error::other)
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

fn read_complete_thread(home: &Path, thread_id: &str) -> Result<Vec<LeadEvent>, String> {
    let text = fs::read_to_string(thread_dir(home, thread_id).join("events.jsonl")).map_err(|e| format!("총괄 대화 기록을 읽지 못했습니다: {e}"))?;
    if !text.ends_with('\n') { return Err("총괄 대화 기록이 불완전하여 카드 질문에 답할 수 없습니다".into()); }
    text.lines().map(|line| serde_json::from_str(line).map_err(|_| "총괄 대화 기록을 완전히 해석할 수 없어 카드 질문에 답할 수 없습니다".to_string())).collect()
}

fn worker_session(events: &[LeadEvent], req: &SendRequest, default_model_supported: bool) -> Result<Option<(String, bool, usize)>, &'static str> {
    if req.model.filter(|model| !model.is_empty()).is_none() && !default_model_supported {
        return Err("기본 모델의 변경 여부를 확인할 수 없어 카드 질문용 새 읽기 전용 세션을 사용합니다");
    }
    let Some((index, event)) = events.iter().enumerate().rev().find(|(_, event)| matches!(event, LeadEvent::WorkerSession { .. })) else {
        return if events.iter().any(|event| matches!(event, LeadEvent::Worker { .. })) {
            Err("이전 카드 질문의 세션 완료를 확인할 수 없어 새 읽기 전용 세션을 사용합니다")
        } else { Ok(None) };
    };
    let LeadEvent::WorkerSession { provider, session_id, model, effort, profile, result_tools, through, history_hash, .. } = event else { unreachable!() };
    if events[index + 1..].iter().any(|event| matches!(event, LeadEvent::Worker { .. })) {
        return Err("이전 카드 질문의 세션 완료를 확인할 수 없어 새 읽기 전용 세션을 사용합니다");
    }
    if *provider != req.provider || model.as_deref() != req.model.filter(|s| !s.is_empty()) || effort.as_deref() != req.effort.filter(|s| !s.is_empty()) || *profile != Profile::of_clean(req.clean.is_some()) {
        return Err("총괄 설정이 달라 카드 질문용 새 읽기 전용 세션을 사용합니다");
    }
    if *through >= index || !matches!(events.get(*through), Some(LeadEvent::Worker { .. }))
        || serde_json::to_string(&events[..index]).ok().is_none_or(|text| crate::run::result::digest(&text) != *history_hash)
        || events.iter().any(|event| matches!(event, LeadEvent::Session { session_id: id, .. } if id == session_id)) {
        return Err("카드 질문 세션의 대화 이력을 확인할 수 없어 새 읽기 전용 세션을 사용합니다");
    }
    Ok(Some((session_id.clone(), *result_tools, index)))
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
    unknown: bool,
}

/// `busy` of a turn whose owner has not written its marker yet.
const STARTING: &str = "starting";

/// How often a running turn looks for a stop request.
const STOP_POLL: Duration = Duration::from_millis(100);

const BUSY: &str = "총괄이 아직 답하는 중입니다. 끝나거나 중지한 뒤 보내세요";
const UNKNOWN: &str = "이전 총괄 프로세스의 종료를 확인할 수 없어 새 총괄을 시작할 수 없습니다";

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

/// A held lock is a live turn; a marker without that lock has an unknown process-tree state.
fn running(home: &Path, thread_id: &str) -> Option<Busy> {
    let lock = match turn_lock(home, thread_id) {
        Ok(lock) => lock,
        Err(_) => return Some(Busy { marker: None, unknown: true }),
    };
    match lock.try_lock() {
        // Free: no turn. Dropping the file lets go of the lock again.
        Ok(()) => return running_path(home, thread_id).exists().then(|| Busy { marker: read_marker(home, thread_id), unknown: true }),
        Err(fs::TryLockError::WouldBlock) => {}
        Err(fs::TryLockError::Error(_)) => return Some(Busy { marker: None, unknown: true }),
    }
    Some(Busy { marker: read_marker(home, thread_id), unknown: false })
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

#[cfg(all(test, windows))]
thread_local! { static FAIL_CHILD_MARKER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

impl TurnSlot {
    fn take(home: &Path, thread_id: &str, provider: ProviderId) -> Result<TurnSlot, String> {
        let lock = turn_lock(home, thread_id).map_err(|e| e.to_string())?;
        // `show` holds the lock for an instant when it looks; only a turn holds it longer.
        let deadline = Instant::now() + Duration::from_millis(300);
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(fs::TryLockError::WouldBlock) if running_path(home, thread_id).exists() => return Err(BUSY.into()),
                Err(fs::TryLockError::WouldBlock) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                Err(fs::TryLockError::WouldBlock) => return Err(BUSY.into()),
                Err(fs::TryLockError::Error(e)) => return Err(format!("총괄 대화를 시작하지 못했습니다: {e}")),
            }
        }
        if running_path(home, thread_id).exists() { return Err(UNKNOWN.into()); }
        // A stop file here is an older turn's: this one has no marker yet, so none was meant for it.
        let _ = fs::remove_file(stop_path(home, thread_id));
        // So are requests on file: their turn ended without closing them.
        approvals::close_left(home, thread_id);
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

    fn set_child(&mut self, pid: u32) -> io::Result<()> {
        #[cfg(all(test, windows))]
        if FAIL_CHILD_MARKER.get() { return Err(io::Error::other("injected child marker failure")); }
        self.marker.child = Some(ChildMark { pid, started: proc::started_at(pid) });
        self.write()
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
    pub thread: Option<&'a str>,
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
    if let Some(thread) = req.thread {
        if req.fresh { return Err("대화 지정과 새 대화를 함께 요청할 수 없습니다".into()); }
        if !is_open(home, req.project, thread) { return Err("이 프로젝트의 열린 총괄 대화가 아닙니다".into()); }
    }
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
    turns_after(events, seen)
}

/// Who a request the overall lead passed on is from, in a project lead's history (ADR-0032).
const FORWARDED: &str = "전체 총괄이 넘긴 요청";
/// Who an overall review turn's request is from, in the overall lead's history (ADR-0036).
const REVIEW_REQUEST: &str = "O의 검토 요청";

/// A thread's speaker for a message not written by the person.
fn origin_label(origin: &str) -> String {
    if origin == ORIGIN_REVIEW { REVIEW_REQUEST } else { FORWARDED }.to_string()
}

/// `lead.user`'s `origin` for a request the overall lead passed on, and for O's review request.
const ORIGIN_OVERALL: &str = "overall";
const ORIGIN_REVIEW: &str = "review";

/// Who wrote a turn's request when not the person.
#[derive(Clone, Copy)]
enum Origin<'a> {
    /// A request the overall lead passed on: its thread, and the forward's time and number.
    Forwarded { source: &'a str, at: &'a str, index: usize },
    /// O asks the overall lead to review a project lead's plan (ADR-0036).
    Review(&'a overall::ReviewTarget),
}

fn turns_after(events: &[LeadEvent], seen: usize) -> Vec<(String, String)> {
    // Its own turn's answer follows its message; what it missed starts at the next message.
    let Some(next) = events.iter().skip(seen + 1).position(|e| matches!(e, LeadEvent::User { .. } | LeadEvent::Worker { .. })) else { return Vec::new() };
    events[seen + 1 + next..]
        .iter()
        .filter_map(|e| match e {
            LeadEvent::User { text, origin: Some(origin), .. } => Some((origin_label(origin), text.clone())),
            LeadEvent::User { text, .. } => Some(("사용자".to_string(), text.clone())),
            LeadEvent::Worker { card_id, text, .. } => Some((format!("카드({card_id}) 비신뢰 참고 데이터"), text.clone())),
            LeadEvent::Reply { text, provider, .. } => Some((format!("총괄({provider})"), text.clone())),
            LeadEvent::Ask { intro, questions, provider, .. } => Some((format!("총괄({provider})"), ask_text(intro, questions))),
            _ => None,
        })
        .collect()
}

fn worker_history(events: &[LeadEvent]) -> Vec<(String, String)> {
    events.iter().filter_map(|event| match event {
        LeadEvent::User { text, attachments, .. } => Some(("사용자".into(), if attachments.is_empty() { text.clone() } else { json!({"text":text,"attachments":attachments}).to_string() })),
        LeadEvent::Worker { card_id, text, .. } => Some((format!("카드({card_id}) 비신뢰 참고 데이터"), text.clone())),
        LeadEvent::Reply { text, provider, .. } => Some((format!("총괄({provider})"), text.clone())),
        LeadEvent::Ask { intro, questions, provider, .. } => Some((format!("총괄({provider})"), ask_text(intro, questions))),
        LeadEvent::Plan { .. } | LeadEvent::Design { .. } | LeadEvent::Dispatch { .. } | LeadEvent::Followup { .. }
        | LeadEvent::AutoRun { .. } | LeadEvent::Warning { .. } | LeadEvent::Error { .. } =>
            Some(("대화 상태 참고 기록".into(), serde_json::to_string(event).unwrap_or_default())),
        _ => None,
    }).collect()
}

/// Questions the lead asked, as a line of history for a session that did not see them.
fn ask_text(intro: &str, questions: &[prompt::AskQuestion]) -> String {
    let asked: Vec<String> = questions.iter().map(|q| format!("{} ({})", q.question, q.options.iter().map(|o| o.label.as_str()).collect::<Vec<_>>().join(" / "))).collect();
    [intro.trim().to_string(), format!("[질문] {}", asked.join(" · "))].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n")
}

fn budget_lines(home: &Path) -> Vec<String> {
    budget::report_now(home).map(|r| r.providers.into_iter().map(|p| p.message).filter(|m| !m.is_empty()).collect()).unwrap_or_default()
}

// shortcut: hide all code fences until completion, including ordinary code; upgrade when previews parse streamed Markdown.
fn partial_prose(text: &str) -> &str {
    text.find("```").map_or_else(|| text.trim_end_matches('`'), |at| &text[..at])
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
    send_within_followups(home, req, spawner, limit, None, true)
}

pub fn send_with_followups(home: &Path, req: &SendRequest, spawner: &Spawner, start: &dyn Fn(&str) -> io::Result<()>) -> Result<Vec<LeadEvent>, String> {
    send_within_followups(home, req, spawner, TURN_LIMIT, Some(start), true)
}

/// `ask`: a person is there to approve actions (ADR-0008 개정 3); a scheduled turn has nobody
/// to ask and stays read-only.
fn send_within_followups(home: &Path, req: &SendRequest, spawner: &Spawner, limit: Duration, start: Option<&StartRun<'_>>, ask: bool) -> Result<Vec<LeadEvent>, String> {
    send_turn(home, req, spawner, limit, start, None, ask, None)
}

/// `o` itself, which serves a turn's approvals (`o _lead-approve`): this program when it is
/// `o`, else the `o` beside it (the desktop app's sidecar). `None`: the lead stays read-only.
fn approval_program() -> Option<PathBuf> {
    if let Some(program) = approvals::program_override() { return Some(program); }
    let current = std::env::current_exe().ok()?;
    if current.file_stem().is_some_and(|stem| stem.eq_ignore_ascii_case("o")) { return Some(current); }
    let beside = current.parent()?.join(if cfg!(windows) { "o.exe" } else { "o" });
    beside.is_file().then_some(beside)
}

fn progress_state(progress: &[Value], counts: &[u64; 4], partial: &str, steps: &[steps::Step]) -> Value {
    json!({ "steps": progress, "counts": { "reads": counts[0], "searches": counts[1], "edits": counts[2], "commands": counts[3] }, "partial": partial, "live": steps })
}

struct WorkerTurn<'a> {
    thread_id: &'a str,
    speaker: &'a team::Speaker,
    message: &'a team::Received,
}

/// Handle one queued card message for the open thread that dispatched this run.
/// `false` means there is no eligible message; unknown deliveries require human recovery.
pub fn wake_card(home: &Path, project: &str, run_id: &str, cards: &[String], spawner: &Spawner) -> Result<bool, String> {
    let speaker = team::Speaker { run_id: run_id.into(), me: "lead".into(), cards: cards.to_vec() };
    let Some(message) = team::read(home, &speaker, false).map_err(|e| e.to_string())?.into_iter().next() else { return Ok(false) };
    // A question the person already answered needs no model turn, even after closing.
    if matches!(message.kind.as_str(), "question" | "approval") && answered(home, &speaker, &message)? {
        team::transition(home, &speaker, &message.id, "acknowledged").map_err(|e| e.to_string())?;
        return Ok(true);
    }
    if message.kind == "approval" { return Err("승인 요청은 사용자의 답변을 기다립니다".into()); }
    if message.state != "stored" { return Err("카드 메시지의 기존 전달 상태를 확인하세요".into()); }
    let Some(thread_id) = run_thread(home, project, run_id) else { return Ok(false) };
    let events = read_complete_thread(home, &thread_id)?;
    let open = {
        let _current = tabs::current_lock(home).map_err(|e| e.to_string())?;
        is_open(home, project, &thread_id)
    };
    if !open {
        let text = format!("분배한 총괄 대화가 닫혀 카드 질문을 전달하지 않았습니다 (실행 {run_id}, 질문 {})", message.id);
        if !events.iter().any(|event| matches!(event, LeadEvent::Warning { text: old, .. } if *old == text)) {
            append(home, &thread_id, &LeadEvent::Warning { at: now_iso(), text: text.clone() }).map_err(|e| e.to_string())?;
        }
        // No answer was sent: preserve it as unresolved and stop automatic retries.
        team::transition(home, &speaker, &message.id, "unknown").map_err(|e| e.to_string())?;
        return Err(text);
    }
    let dispatch = events.iter().rposition(|event| matches!(event,
        LeadEvent::Dispatch { run_id: id, .. } | LeadEvent::Followup { run_id: id, .. } if id == run_id)).unwrap_or(events.len());
    let Some((provider, model, effort, profile)) = events[..dispatch].iter().rev().find_map(|event| match event {
        LeadEvent::User { provider, model, effort, profile, .. } => Some((*provider, model.as_deref(), effort.as_deref(), *profile)),
        _ => None,
    }) else { return Err("실행에 연결된 총괄 설정이 없습니다".into()) };
    let Some(profile) = profile else { return Err("이전 총괄의 프로필이 기록되지 않아 자동 응답할 수 없습니다".into()) };
    // An effort recorded before the CLI stopped taking it (Codex `minimal`) runs at the CLI's default.
    let effort = effort.filter(|e| providers::get(provider).lead_efforts().contains(e));
    let root = clean::default_root();
    let req = SendRequest { project, thread: Some(&thread_id), provider, model, effort, text: &message.text, attach: &[], fresh: false, clean: profile.is_clean().then_some(root.as_path()), yolo: false };
    send_turn(home, &req, spawner, TURN_LIMIT, None, Some(WorkerTurn { thread_id: &thread_id, speaker: &speaker, message: &message }), false, None)?;
    if !team::read(home, &speaker, true).map_err(|e| e.to_string())?.iter().any(|m| m.id == message.id && m.state == "acknowledged") {
        return Err("총괄 처리 결과가 불명입니다. 대화와 메시지 상태를 확인하세요".into());
    }
    Ok(true)
}

/// The lead (or the person as the lead) has already replied to this question.
fn answered(home: &Path, speaker: &team::Speaker, message: &team::Received) -> Result<bool, String> {
    Ok(team::read(home, speaker, true).map_err(|e| e.to_string())?.iter().any(|m| m.from == "lead" && m.to == message.from && m.reply_to.as_deref() == Some(&message.id)))
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
    let thread_id = run_thread(home, &plan.project, run_id).ok_or("실행을 분배한 총괄 대화가 없습니다")?;
    let current = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(leads_dir(home).join("current.lock")).map_err(|e| e.to_string())?;
    current.lock().map_err(|e| e.to_string())?;
    if !is_open(home, &plan.project, &thread_id) { return Err("분배한 총괄 대화가 닫혔습니다".into()); }
    let turn = turn_lock(home, &thread_id).map_err(|e| e.to_string())?;
    turn.try_lock().map_err(|_| "총괄이 응답 중입니다. 완료 후 상태를 확인하세요")?;
    if running_path(home, &thread_id).exists() { return Err(UNKNOWN.into()); }
    let speaker = team::Speaker { run_id: run_id.into(), me: "lead".into(), cards: plan.cards.iter().map(|card| card.id.clone()).collect() };
    let pending = team::read(home, &speaker, false).map_err(|e| e.to_string())?;
    let message = pending.first().filter(|m| m.id == message_id && matches!(m.state.as_str(), "unknown" | "delivered")).ok_or("첫 번째 처리 불명 메시지를 확인하세요")?;
    let sent = team::read(home, &speaker, true).map_err(|e| e.to_string())?.iter().any(|m| m.from == "lead" && m.to == message.from && m.reply_to.as_deref() == Some(message_id));
    let state = if sent { "acknowledged" } else { "stored" };
    team::transition(home, &speaker, message_id, state).map_err(|e| e.to_string())?;
    Ok(if sent { "acknowledged" } else { "queued" })
}

/// A request the overall lead passed on (`overall::forward`), recorded as such in the project's thread.
fn send_forwarded(home: &Path, req: &SendRequest, spawner: &Spawner, source: &str, at: &str, index: usize) -> Result<Vec<LeadEvent>, String> {
    if overall::is_overall(home, req.project) { return Err("전체 총괄 자신에게는 넘기지 않습니다".into()); }
    send_turn(home, req, spawner, TURN_LIMIT, None, None, false, Some(Origin::Forwarded { source, at, index }))
}

/// An overall review turn (ADR-0036): read-only, its request written by O.
fn send_review(home: &Path, req: &SendRequest, spawner: &Spawner, target: &overall::ReviewTarget) -> Result<Vec<LeadEvent>, String> {
    if !overall::is_overall(home, req.project) { return Err("전체 총괄 대화가 아닙니다".into()); }
    send_turn(home, req, spawner, TURN_LIMIT, None, None, false, Some(Origin::Review(target)))
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

    #[test]
    fn worker_session_requires_matching_settings_complete_history_and_no_unfinished_turn() {
        let mut events: Vec<LeadEvent> = [
            json!({"t":"lead.start","at":"now","threadId":"thread","project":"p"}),
            json!({"t":"lead.worker","at":"now","runId":"run","cardId":"card","messageId":"m","text":"question"}),
        ].into_iter().map(|value| serde_json::from_value(value).unwrap()).collect();
        let hash = crate::run::result::digest(&serde_json::to_string(&events).unwrap());
        events.push(LeadEvent::WorkerSession { at: "now".into(), provider: ProviderId::Claude, session_id: "worker".into(), model: Some("model-a".into()), effort: Some("high".into()), profile: Profile::Own, result_tools: false, through: 1, history_hash: hash });
        let req = SendRequest { project: "p", thread: None, provider: ProviderId::Claude, model: Some("model-a"), effort: Some("high"), text: "question", attach: &[], fresh: false, clean: None, yolo: false };
        assert_eq!(worker_session(&events, &req, false).unwrap().unwrap().0, "worker");
        assert!(worker_session(&events, &SendRequest { model: Some("model-b"), ..req }, false).is_err());
        assert!(worker_session(&events, &SendRequest { model: None, ..req }, false).is_err());
        assert!(worker_session(&events, &SendRequest { provider: ProviderId::Codex, ..req }, false).is_err());
        assert!(worker_session(&events, &SendRequest { clean: Some(Path::new("other")), ..req }, false).is_err());
        assert!(worker_session(&events, &SendRequest { effort: None, ..req }, false).is_err());
        let mut damaged = events.clone();
        damaged.remove(0);
        assert!(worker_session(&damaged, &req, false).is_err());
        let mut interrupted = events.clone();
        interrupted.push(serde_json::from_value(json!({"t":"lead.worker","at":"now","runId":"run","cardId":"card","messageId":"m2","text":"unfinished"})).unwrap());
        assert!(worker_session(&interrupted, &req, false).is_err());
        let mut collision = events;
        collision.push(serde_json::from_value(json!({"t":"lead.session","at":"now","provider":"claude","sessionId":"worker","clean":false})).unwrap());
        assert!(worker_session(&collision, &req, false).is_err());

        let mut default_events = collision;
        default_events.pop();
        if let LeadEvent::WorkerSession { provider, model, .. } = default_events.last_mut().unwrap() {
            *provider = ProviderId::Codex;
            *model = None;
        }
        let default_req = SendRequest { provider: ProviderId::Codex, model: None, ..req };
        assert!(worker_session(&default_events, &default_req, false).is_err());
        assert_eq!(worker_session(&default_events, &default_req, true).unwrap().unwrap().0, "worker");
    }
}
