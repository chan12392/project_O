//! Reads lead conversation status, sessions, and token totals.

use super::*;

/// Stop one addressed thread, a project's open threads, or every open thread.
pub fn stop(home: &Path, project: Option<&str>, thread: Option<&str>) -> Result<bool, String> {
    let _lock = tabs::current_lock(home).map_err(|e| e.to_string())?;
    let current = read_current(home).map_err(|e| e.to_string())?;
    let projects = tabs::read_open(home, &current).map_err(|e| e.to_string())?;
    let threads = if let Some(id) = thread {
        if !is_run_id(id) || !projects.iter().any(|(key, tabs)| project.is_none_or(|p| *key == project_key(p)) && tabs.open.iter().any(|open| open == id)) {
            return Err("이 프로젝트의 열린 총괄 대화가 아닙니다".into());
        }
        vec![id.to_string()]
    } else {
        projects.into_iter().filter(|(key, _)| project.is_none_or(|p| *key == project_key(p)))
            .flat_map(|(_, tabs)| tabs.open).filter(|id| is_run_id(id)).collect()
    };
    let mut stopped = false;
    for thread_id in threads {
        stopped |= stop_thread(home, &thread_id);
    }
    Ok(stopped)
}

fn stop_thread(home: &Path, thread_id: &str) -> bool {
    if let Some(busy) = running(home, thread_id) {
        if busy.unknown { return false; }
        // Only the turn's owner ends its CLI; never kill an owner or trust an old PID.
        let mut marker = busy.marker;
        for _ in 0..20 {
            if marker.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
            marker = read_marker(home, thread_id);
        }
        let _ = fs::write(stop_path(home, thread_id), marker.map(|m| m.token).unwrap_or_default());
        return true;
    }
    false
}

/// Stop every open lead and live run in this project.
pub fn stop_all(home: &Path, project: &str) -> Result<usize, String> {
    let current = tabs::current_lock(home).map_err(|e| e.to_string())?;
    let tabs = tabs::read_open(home, &read_current(home).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let key = project_key(project);
    let mut count = 0;
    for thread in tabs.get(&key).into_iter().flat_map(|tabs| &tabs.open).filter(|id| is_run_id(id)) {
        let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(thread_dir(home, thread).join("dispatch.lock")).map_err(|e| e.to_string())?;
        lock.lock().map_err(|e| e.to_string())?;
        fs::write(thread_dir(home, thread).join("stop-all-at"), Utc::now().to_rfc3339()).map_err(|e| e.to_string())?;
        count += usize::from(stop_thread(home, thread));
    }
    drop(current);
    for run_id in store::list_runs(home).into_iter().filter(|id| is_run_id(id)) {
        let Ok(plan) = store::read_plan(home, &run_id) else { continue };
        let Some(run_project) = plan["project"].as_str() else { continue };
        if project_key(run_project) == key && store::run_is_live(home, &run_id)
            && crate::run::runner::stop_run(home, &run_id).map_err(|e| e.to_string())? { count += 1; }
    }
    Ok(count)
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
    /// This turn's steps so far, running ones included (`steps::Step`); the finished ones are
    /// also `lead.step` events.
    pub steps: Vec<Value>,
    /// Requests waiting for the person's 허용/거부, oldest first (`approvals::pending`).
    pub approvals: Vec<Value>,
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
    pub state: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeadTab {
    pub thread_id: String,
    pub title: String,
    pub state: String,
    pub busy: Option<String>,
    pub writer: bool,
    pub started_at: String,
}

fn tab_view(home: &Path, id: &str, events: &[LeadEvent], writer: Option<&str>, live: &mut BTreeMap<String, bool>, selected: Option<&ThreadView>) -> LeadTab {
    let busy = selected.map(|view| view.busy.clone()).unwrap_or_else(|| running(home, id)
        .map(|b| if b.unknown { "unknown".into() } else { b.marker.map_or_else(|| STARTING.into(), |m| m.provider.as_str().into()) }));
    let pending_approval = selected.filter(|view| view.busy.is_some()).map(|view| !view.approvals.is_empty())
        .unwrap_or_else(|| !approvals::pending(home, id).is_empty());
    let asked = events.iter().rev().find_map(|event| match event { LeadEvent::Ask { .. } => Some(true), LeadEvent::User { .. } => Some(false), _ => None }).unwrap_or(false);
    let plan = events.iter().rev().find_map(|event| match event { LeadEvent::Plan { plan_id, ok, .. } => Some((plan_id, ok)), _ => None });
    let pending_plan = plan.is_some_and(|(id, ok)| *ok && !events.iter().any(|event| matches!(event, LeadEvent::Dispatch { plan_id, .. } if plan_id == id)));
    // The overall lead's latest proposal not sent on yet, or with a request that did not start (ADR-0036).
    let pending_proposal = events.iter().rev().find_map(|event| match event { LeadEvent::ForwardPlan { proposal_id, .. } => Some(Some(proposal_id)), LeadEvent::User { .. } => Some(None), _ => None })
        .flatten().is_some_and(|id| {
            let unstarted = overall::unstarted(events);
            let mut sent = events.iter().filter(|event| matches!(event, LeadEvent::Forward { .. })).enumerate()
                .filter(|(_, event)| matches!(event, LeadEvent::Forward { proposal: Some(p), .. } if p == id)).peekable();
            sent.peek().is_none() || sent.any(|(n, _)| unstarted.contains(&n))
        });
    // shortcut: display checks eight recent runs; use an active-run index if older live runs must stay visible.
    let state = if busy.as_deref() == Some("unknown") || asked || pending_plan || pending_proposal || pending_approval { "waiting" }
        else if busy.is_some() || events.iter().rev().filter_map(|event| match event {
            LeadEvent::Dispatch { run_id, .. } | LeadEvent::Followup { run_id, .. } => Some(run_id),
            _ => None,
        }).take(8).any(|run_id| is_run_id(run_id) && *live.entry(run_id.clone())
            .or_insert_with(|| store::runner_may_be_live(home, run_id) && store::run_is_live(home, run_id))) { "working" } else { "idle" };
    LeadTab {
        thread_id: id.into(),
        title: events.iter().find_map(|event| match event { LeadEvent::User { text, .. } => Some(text.lines().next().unwrap_or_default().chars().take(24).collect()), _ => None }).unwrap_or_default(),
        state: state.into(), busy, writer: writer == Some(id),
        started_at: events.iter().find_map(|event| match event { LeadEvent::Start { at, .. } => Some(at.clone()), _ => None }).unwrap_or_default(),
    }
}

/// Addressed or current conversation, its open tabs, and aggregate project states.
pub fn show(home: &Path, project: Option<&str>, requested: Option<&str>) -> Result<Value, String> {
    if requested.is_some_and(|id| !is_run_id(id)) { return Err("총괄 대화 ID가 올바르지 않습니다".into()); }
    let lock = tabs::current_lock(home).map_err(|e| e.to_string())?;
    let current = read_current(home).map_err(|e| e.to_string())?;
    let open = tabs::read_open(home, &current).map_err(|e| e.to_string())?;
    drop(lock);
    let history = |id: &str| {
        let values = event_values(home, id);
        let events: Vec<LeadEvent> = values.iter().filter_map(|value| serde_json::from_value(value.clone()).ok()).collect();
        (values, events)
    };
    let mut requested_history = requested.map(history);
    let owner = requested_history.as_ref().and_then(|(_, events)| thread_project(events));
    if let (Some(project), Some(owner)) = (project, owner.as_deref()) {
        if project_key(project) != project_key(owner) { return Err("이 프로젝트의 총괄 대화가 아닙니다".into()); }
    }
    let selected_key = project.or(owner.as_deref()).map(project_key).or_else(|| current.iter()
        .filter(|(_, id)| is_run_id(id)).max_by_key(|(_, id)| *id).map(|(key, _)| key.clone()));
    let thread_id = selected_key.as_ref().and_then(|key| {
        let tabs = open.get(key)?;
        requested.filter(|id| tabs.open.iter().any(|open| open == id)).map(str::to_string)
            .or_else(|| current.get(key).filter(|id| is_run_id(id)).cloned())
    });
    let mut thread = None;
    let mut threads = Vec::new();
    let mut projects = Vec::new();
    let mut live = BTreeMap::new();
    for (key, tabs) in &open {
        let mut name = None;
        let mut state = "idle";
        for id in tabs.open.iter().filter(|id| is_run_id(id)) {
            let (values, events) = if requested == Some(id.as_str()) { requested_history.take().unwrap() } else { history(id) };
            if current.get(key) == Some(id) { name = thread_project(&events); }
            if thread_id.as_ref() == Some(id) {
                let (notes, notes_error) = match notes::read(home, id) { Ok(notes) => (notes, None), Err(error) => (Vec::new(), Some(error.to_string())) };
                let busy = running(home, id).map(|b| if b.unknown { "unknown".to_string() } else { b.marker.map_or_else(|| STARTING.to_string(), |m| m.provider.as_str().to_string()) });
                let progress: Value = busy.as_ref().and_then(|_| fs::read(thread_dir(home, id).join("progress.json")).ok())
                    .and_then(|data| serde_json::from_slice(&data).ok()).unwrap_or_default();
                let approvals = if busy.as_deref().is_some_and(|b| b != "unknown") { approvals::pending(home, id) } else { Vec::new() };
                thread = Some(ThreadView {
                    project: thread_project(&events).unwrap_or_default(), busy,
                    progress: progress.get("steps").or_else(|| progress.is_array().then_some(&progress)).and_then(Value::as_array).cloned().unwrap_or_default(),
                    progress_counts: progress.get("counts").cloned().unwrap_or(Value::Null),
                    partial: progress.get("partial").and_then(Value::as_str).unwrap_or_default().to_string(),
                    steps: progress.get("live").and_then(Value::as_array).cloned().unwrap_or_default(),
                    approvals, notes, notes_error, events: values, thread_id: id.clone(),
                });
            }
            let tab = tab_view(home, id, &events, tabs.writer.as_deref(), &mut live, thread.as_ref().filter(|view| view.thread_id == *id));
            if tab.state == "waiting" { state = "waiting"; }
            else if tab.state == "working" && state == "idle" { state = "working"; }
            if selected_key.as_ref() == Some(key) { threads.push(tab); }
        }
        if let (Some(project), Some(id)) = (name, current.get(key)) {
            projects.push(LeadProject { project, thread_id: id.clone(), state: state.into() });
        }
    }
    projects.sort_by(|a, b| b.thread_id.cmp(&a.thread_id));
    Ok(json!({ "thread": thread, "threads": threads, "projects": projects }))
}

pub(super) fn thread_ids(home: &Path) -> Vec<String> {
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
            Ok(()) => { if running_path(home, &id).exists() { count += 1; } },
            Err(fs::TryLockError::WouldBlock) => count += 1,
            Err(fs::TryLockError::Error(error)) => return Err(error),
        }
    }
    Ok(count)
}

pub(super) fn sessions_path(home: &Path, thread_id: &str) -> PathBuf {
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
            LeadEvent::Session { session_id, .. } | LeadEvent::WorkerSession { session_id, .. } => Some(session_id),
            _ => None,
        })
        .collect();
    if let Ok(mut file) = OpenOptions::new().write(true).create_new(true).open(&path) {
        let _ = file.write_all(ids.iter().map(|id| format!("{id}\n")).collect::<String>().as_bytes());
    }
    ids
}

/// Adds a session id to its thread's index (made from the events first if the thread is older).
pub(super) fn index_session(home: &Path, thread_id: &str, session_id: &str) {
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
        let raw = read_thread(home, &id);
        let values: Vec<_> = raw.iter().filter_map(|e| serde_json::to_value(e).ok()).collect();
        let events: Vec<LeadEvent> = crate::usage::accounting::project(&values).into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect();
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
