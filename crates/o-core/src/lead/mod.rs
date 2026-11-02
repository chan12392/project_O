//! The lead chat (ADR-0008): a person writes to a project's lead, an official CLI
//! reads the project (read-only) and proposes a work-card plan, and only the
//! person's "실행" makes O dispatch it, through the same policy as `o dispatch`.
//!
//! ```text
//! leads/current.json                 project (normalized) → its current thread id
//! leads/<threadId>/events.jsonl      the thread: lead.start · lead.user · lead.session · lead.reply
//!                                    · lead.plan · lead.dispatch · lead.usage · lead.error
//! leads/<threadId>/turn.raw.jsonl    the CLIs' stdout, for reading back a turn
//! leads/<threadId>/running.json      the turn in progress ({ pid, provider }); stop: its stop request
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
use std::thread;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agents::clean;
use crate::context::normalize_project;
use crate::policy::{check_plan, check_project, Violation, LIMITS};
use crate::providers::{self, LeadTurn, StreamReader};
use crate::run::events::StreamSignal;
use crate::run::plan::ProviderId;
use crate::run::runner::Spawner;
use crate::run::snapshot::snapshot;
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
    },
    /// The CLI's own session id, resumed on the thread's next turn with that CLI.
    #[serde(rename = "lead.session")]
    Session { at: String, provider: ProviderId, session_id: String },
    /// The lead's answer as Markdown, without its plan block.
    #[serde(rename = "lead.reply")]
    Reply { at: String, provider: ProviderId, text: String },
    /// A proposed plan with the project forced to the thread's and the policy check's result.
    #[serde(rename = "lead.plan")]
    Plan { at: String, plan_id: String, plan: Value, ok: bool, violations: Vec<Violation> },
    #[serde(rename = "lead.dispatch")]
    Dispatch { at: String, plan_id: String, run_id: String, clean: bool },
    /// Tokens one lead turn spent (`o tokens` counts them as planning overhead).
    #[serde(rename = "lead.usage")]
    Usage { at: String, provider: ProviderId, input: f64, cached: f64, output: f64 },
    #[serde(rename = "lead.error")]
    Error { at: String, text: String },
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

fn read_current(home: &Path) -> BTreeMap<String, String> {
    fs::read_to_string(leads_dir(home).join("current.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn write_current(home: &Path, current: &BTreeMap<String, String>) -> io::Result<()> {
    fs::create_dir_all(leads_dir(home))?;
    fs::write(leads_dir(home).join("current.json"), store::to_pretty(current)?)
}

/// The project's current thread, if it has one.
pub fn current_thread(home: &Path, project: &str) -> Option<String> {
    read_current(home).get(&project_key(project)).filter(|id| is_run_id(id)).cloned()
}

/// Starts a new thread for the project and makes it the current one.
pub fn new_thread(home: &Path, project: &str) -> io::Result<String> {
    let thread_id = new_run_id(Utc::now());
    fs::create_dir_all(thread_dir(home, &thread_id))?;
    append(home, &thread_id, &LeadEvent::Start { at: now_iso(), thread_id: thread_id.clone(), project: project.to_string() })?;
    let mut current = read_current(home);
    current.insert(project_key(project), thread_id.clone());
    write_current(home, &current)?;
    Ok(thread_id)
}

/// The project's current thread, or a new one when it has none or `fresh` asks for it.
pub fn thread_for(home: &Path, project: &str, fresh: bool) -> io::Result<String> {
    match current_thread(home, project) {
        Some(id) if !fresh => Ok(id),
        _ => new_thread(home, project),
    }
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

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Running {
    pid: u32,
    provider: ProviderId,
}

fn running_path(home: &Path, thread_id: &str) -> PathBuf {
    thread_dir(home, thread_id).join("running.json")
}

/// The live turn of a thread, if any; a marker whose process is gone does not count.
fn running(home: &Path, thread_id: &str) -> Option<Running> {
    let marker: Running = serde_json::from_str(&fs::read_to_string(running_path(home, thread_id)).ok()?).ok()?;
    proc::is_alive(marker.pid).then_some(marker)
}

/// Holds a thread's one turn: made with `create_new`, so two sends cannot both start; removed when dropped.
struct TurnSlot {
    path: PathBuf,
}

impl TurnSlot {
    fn take(home: &Path, thread_id: &str, provider: ProviderId) -> Result<TurnSlot, String> {
        let path = running_path(home, thread_id);
        for _ in 0..2 {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    let marker = serde_json::to_string(&Running { pid: std::process::id(), provider }).map_err(|e| e.to_string())?;
                    file.write_all(marker.as_bytes()).map_err(|e| e.to_string())?;
                    return Ok(TurnSlot { path });
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                    if running(home, thread_id).is_some() {
                        return Err("총괄이 아직 답하는 중입니다. 끝나거나 중지한 뒤 보내세요".into());
                    }
                    // Left by a turn whose process is gone.
                    let _ = fs::remove_file(&path);
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        Err("총괄 대화를 시작하지 못했습니다".into())
    }

    fn set_pid(&self, pid: u32, provider: ProviderId) {
        if let Ok(text) = serde_json::to_string(&Running { pid, provider }) {
            let _ = fs::write(&self.path, text);
        }
    }
}

impl Drop for TurnSlot {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
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
    /// The clean profile root when the lead runs as a clean agent (Settings "깨끗한 에이전트").
    pub clean: Option<&'a Path>,
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
        if !model.chars().all(|c| c.is_ascii_alphanumeric() || "._:-/[]".contains(c)) || model.len() > 80 {
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

fn budget_lines(home: &Path) -> Vec<String> {
    budget::report_now(home).map(|r| r.providers.into_iter().map(|p| p.message).filter(|m| !m.is_empty()).collect()).unwrap_or_default()
}

/// Sends one message: saves it and its attachments, runs the lead CLI read-only in the
/// project (resuming its session), and records the reply, the plan with its policy
/// result, the tokens and any error. Returns the events this turn added.
pub fn send(home: &Path, req: &SendRequest, spawner: &Spawner) -> Result<Vec<LeadEvent>, String> {
    check_request(home, req)?;
    let thread_id = thread_for(home, req.project, req.fresh).map_err(|e| e.to_string())?;
    let slot = TurnSlot::take(home, &thread_id, req.provider)?;
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
    })?;

    let session = before.iter().rev().find_map(|e| match e {
        LeadEvent::Session { provider, session_id, .. } if *provider == req.provider => Some(session_id.clone()),
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
    let Some(spec) = providers::get(req.provider).lead(&turn) else {
        return Err(format!("{}는 총괄로 쓸 수 없습니다", req.provider));
    };

    let dir = thread_dir(home, &thread_id);
    let _ = fs::remove_file(dir.join("stop"));
    let spawned = spawner(&spec, req.project).and_then(|mut command| {
        if let Some(root) = req.clean {
            command.env_clear().envs(clean::environment(root, req.provider)?);
        }
        // An inherited API key would bill the API instead of the subscription the CLI signed in with.
        providers::remove_inherited(&mut command, &spec.env_remove);
        command.envs(&spec.env);
        proc::hide_window(&mut command).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()
    });
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            emit(LeadEvent::Error { at: now_iso(), text: format!("총괄 CLI를 시작하지 못했습니다: {error}") })?;
            return Ok(added);
        }
    };
    let _tree = proc::contain(&child);
    slot.set_pid(child.id(), req.provider);
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
                            emit(LeadEvent::Session { at: now_iso(), provider: req.provider, session_id })?;
                        }
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
    let stderr_tail = stderr.and_then(|h| h.join().ok()).unwrap_or_default();
    let exit = child.wait().ok().and_then(proc::exit_code);
    if used {
        emit(LeadEvent::Usage { at: now_iso(), provider: req.provider, input, cached, output })?;
    }

    if dir.join("stop").exists() {
        emit(LeadEvent::Error { at: now_iso(), text: "총괄을 중지했습니다".into() })?;
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
            let (plan, violations) = check_lead_plan(home, req.project, plan);
            emit(LeadEvent::Plan { at: now_iso(), plan_id: format!("{thread_id}-p{}", plans + 1), plan, ok: violations.is_empty(), violations })?;
        }
        Err(message) => emit(LeadEvent::Error { at: now_iso(), text: message })?,
    }
    Ok(added)
}

/// A plan from the lead with its project forced to the thread's, and what the policy says
/// of it now (`o dispatch`'s check: shape, providers, caps, cooldowns, today's budget).
pub fn check_lead_plan(home: &Path, project: &str, mut plan: Value) -> (Value, Vec<Violation>) {
    if let Some(object) = plan.as_object_mut() {
        object.insert("project".into(), Value::String(project.to_string()));
    }
    let violations = match policy_context(home, Utc::now()) {
        Ok(ctx) => check_plan(&plan, &ctx).violations,
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
    pub clean: bool,
}

/// Dispatches a stored plan once: checks it again like `o dispatch`, makes the run and
/// hands its id to `start` (which launches `o _run <runId>` in the background).
pub fn dispatch(home: &Path, plan_id: &str, clean: bool, start: &dyn Fn(&str) -> io::Result<()>) -> Result<Dispatched, DispatchError> {
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
    let checked = check_plan(&plan, &ctx);
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
        .and_then(|()| if clean { store::mark_clean(home, &run_id) } else { Ok(()) })
        .and_then(|()| start(&run_id));
    if let Err(error) = started {
        // Nothing runs: the plan may be dispatched again.
        let _ = fs::remove_file(&mark);
        return Err(format!("실행기를 시작하지 못했습니다: {error}").into());
    }
    append(home, thread_id, &LeadEvent::Dispatch { at: now_iso(), plan_id: plan_id.to_string(), run_id: run_id.clone(), clean }).map_err(|e| e.to_string())?;
    Ok(Dispatched {
        plan_id: plan_id.to_string(),
        events: run_dir(home, &run_id).join("events.jsonl").to_string_lossy().into_owned(),
        run_id,
        cards: plan.cards.iter().map(|c| c.id.clone()).collect(),
        clean,
    })
}

/// Stops the lead turn of the project's current thread (every live turn without a
/// project): a stop request, then the CLI's process tree. `false` when none was running.
pub fn stop(home: &Path, project: Option<&str>) -> bool {
    let threads: Vec<String> = match project {
        Some(project) => current_thread(home, project).into_iter().collect(),
        None => read_current(home).into_values().filter(|id| is_run_id(id)).collect(),
    };
    let mut stopped = false;
    for thread_id in threads {
        if let Some(marker) = running(home, &thread_id) {
            let _ = fs::write(thread_dir(home, &thread_id).join("stop"), "");
            // The marker names this process until the CLI starts; only a child is ended.
            if marker.pid != std::process::id() {
                proc::kill_tree(marker.pid);
            }
            stopped = true;
        }
    }
    stopped
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadView {
    pub thread_id: String,
    pub project: String,
    /// A turn is running; its provider.
    pub busy: Option<ProviderId>,
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
    let current = read_current(home);
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
        busy: running(home, &id).map(|r| r.provider),
        events: event_values(home, &id),
        thread_id: id,
    });
    json!({ "thread": thread, "projects": projects })
}

fn thread_ids(home: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(leads_dir(home)) else { return Vec::new() };
    entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|id| is_run_id(id)).collect()
}

/// Every CLI session a lead turn used, so `o runs --sessions` does not list the lead as separate work.
pub fn session_ids(home: &Path) -> Vec<String> {
    thread_ids(home)
        .iter()
        .flat_map(|id| read_thread(home, id))
        .filter_map(|e| match e {
            LeadEvent::Session { session_id, .. } => Some(session_id),
            _ => None,
        })
        .collect()
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
