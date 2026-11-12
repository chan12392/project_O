//! Validates and dispatches plans proposed by a lead.

use super::*;

/// A plan from the lead with its project forced to the thread's, and what the policy says
/// of it now (`o dispatch`'s check: shape, providers, caps, cooldowns, today's budget, and
/// the profile the lead ran on, which the app dispatches the plan with).
pub fn check_lead_plan(home: &Path, project: &str, mut plan: Value, mode: RunMode) -> (Value, Vec<Violation>, Vec<String>) {
    if let Some(object) = plan.as_object_mut() {
        object.insert("project".into(), Value::String(project.to_string()));
    }
    let (violations, notices) = match policy_context(home, Utc::now()) {
        Ok(ctx) => { let checked = check_plan_in(&plan, &ctx, mode); (checked.violations, checked.notices) },
        Err(error) => (vec![Violation { rule: "policy-context".into(), message: error.to_string(), card_id: None }], Vec::new()),
    };
    (plan, violations, notices)
}

pub(super) fn check_generated_plan(home: &Path, project: &str, plan: Value, mode: RunMode) -> (Value, Vec<Violation>, Vec<String>) {
    let (plan, mut violations, notices) = check_lead_plan(home, project, plan, mode);
    for card in plan["cards"].as_array().into_iter().flatten() {
        let Some(provider) = card["provider"].as_str().and_then(ProviderId::parse) else { continue };
        if providers::get(provider).terminal_card() { continue; }
        let model = card["model"].as_str().filter(|s| !s.is_empty());
        let effort = card["effort"].as_str().filter(|s| !s.is_empty());
        let error = if model.is_none() || effort.is_none() { Some("새 총괄 계획의 헤드리스 카드는 model과 effort를 명시해야 합니다".into()) }
            else {
                let root = clean::default_root();
                providers::models::cached(home, provider, mode.profile.is_clean().then_some(root.as_path()), Path::new(project)).and_then(|c| providers::models::validate(&c, model.unwrap(), effort).err())
            };
        if let Some(message) = error { violations.push(Violation { rule: "model-selection".into(), message, card_id: card["id"].as_str().map(str::to_owned) }); }
    }
    (plan, violations, notices)
}

#[cfg(test)]
#[test]
fn generated_headless_plans_require_explicit_choices_but_legacy_and_terminal_plans_do_not() {
    let dir = std::env::temp_dir().join(format!("o-generated-model-{}", store::new_run_id(Utc::now())));
    let project = dir.join("project");
    fs::create_dir_all(&project).unwrap();
    let home = dir.join("state");
    let project = project.to_string_lossy();
    let mode = RunMode::of(Profile::Own);
    let mut plan = serde_json::json!({"title":"fixture","cards":[{"id":"a","provider":"claude","task":"inspect"}]});
    assert!(check_generated_plan(&home, &project, plan.clone(), mode).1.iter().any(|v| v.rule == "model-selection"));
    assert!(!check_lead_plan(&home, &project, plan.clone(), mode).1.iter().any(|v| v.rule == "model-selection"));
    plan["cards"][0]["model"] = serde_json::json!("explicit-model");
    plan["cards"][0]["effort"] = serde_json::json!("medium");
    assert!(!check_generated_plan(&home, &project, plan, mode).1.iter().any(|v| v.rule == "model-selection"));
    let terminal = serde_json::json!({"title":"fixture","cards":[{"id":"a","provider":"agy","task":"inspect"}]});
    assert!(!check_generated_plan(&home, &project, terminal, mode).1.iter().any(|v| v.rule == "model-selection"));
}

/// `<threadId>-p<n>` → the thread id.
pub(super) fn plan_thread(plan_id: &str) -> Option<&str> {
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
    dispatch_locked(home, plan_id, mode, start, true, false)
}

/// The locks every dispatch (by hand or automatic) holds while it checks and starts a plan.
pub(super) struct DispatchLocks {
    _current: fs::File,
    _turn: fs::File,
    _dispatch: fs::File,
}

/// Takes the dispatch locks in their one order (the thread list, the thread's turn, its
/// dispatch) and checks that the plan's thread is still the project's current one, that
/// no turn is answering, and that the plan is still the thread's latest word.
pub(super) fn lock_latest(home: &Path, plan_id: &str) -> Result<DispatchLocks, String> {
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
    if running_path(home, thread_id).exists() { return Err(UNKNOWN.into()); }
    let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(thread_dir(home, thread_id).join("dispatch.lock")).map_err(|e| e.to_string())?;
    lock.lock().map_err(|e| e.to_string())?;
    let events = read_thread(home, thread_id);
    let project = thread_project(&events).ok_or_else(|| "총괄 대화를 찾지 못했습니다".to_string())?;
    let latest = events.iter().rev().find_map(|event| match event {
        LeadEvent::Plan { plan_id, .. } => Some(Some(plan_id.as_str())),
        LeadEvent::User { .. } => Some(None),
        _ => None,
    }).flatten();
    if !is_open(home, &project, thread_id) || latest != Some(plan_id) {
        return Err("설계가 변경되었습니다. 최신 설계를 확인한 뒤 분배하세요".to_string());
    }
    Ok(DispatchLocks { _current: current, _turn: turn, _dispatch: lock })
}

fn dispatch_locked(home: &Path, plan_id: &str, mode: RunMode, start: &dyn Fn(&str) -> io::Result<()>, retry: bool, scheduled: bool) -> Result<Dispatched, DispatchError> {
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
    let checked = check_plan_in(&plan, &ctx, mode);
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
        .and_then(|()| if scheduled { store::mark_scheduled(home, &run_id) } else { Ok(()) })
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
    let mut added = send_within_followups(home, req, spawner, TURN_LIMIT, resume, !scheduled)?;
    let Some((plan_id, plan, ok)) = added.iter().find_map(|e| match e {
        LeadEvent::Plan { plan_id, plan, ok, .. } => Some((plan_id.clone(), plan.clone(), *ok)),
        _ => None,
    }) else { return Ok(added) };
    // The same locks and checks as a dispatch by hand: closing, a newer request or turn wins.
    // Nothing is written then, since another turn may own the thread.
    let _locks = match lock_latest(home, &plan_id) {
        Ok(locks) => locks,
        Err(why) => {
            added.push(LeadEvent::AutoRun { at: now_iso(), plan_id, hash: None, reason: Some(format!("자동 실행을 취소했습니다: {why}")) });
            return Ok(added);
        }
    };
    let scheduled_agy = scheduled && plan.get("cards").and_then(Value::as_array).is_some_and(|cards| cards.iter().any(|card| card.get("provider").and_then(Value::as_str) == Some("agy")));
    let wait = if scheduled_agy { Some("예약에서는 Antigravity를 실행할 수 없습니다".to_string()) }
        else { (!req.yolo && !scheduled).then(|| "YOLO 모드가 꺼져 있어 실행을 기다립니다".to_string()) };
    let mode = RunMode { yolo: req.yolo, ..RunMode::of(Profile::of_clean(req.clean.is_some())) };
    added.extend(auto_dispatch(home, &plan_id, &plan, ok, mode, turn_started, wait, start, scheduled)?);
    Ok(added)
}

/// The overall lead's verdict on a project lead's plan (ADR-0036), written into the plan's
/// thread under the dispatch locks (so only while the plan is still that thread's latest word).
/// `approve` then runs it there and then, like 실행, within the auto-run limits; a stop of the
/// thread since `since` cancels it.
pub(super) fn apply_review(home: &Path, review: &LeadEvent, profile: Profile, since: DateTime<Utc>, start: &dyn Fn(&str) -> io::Result<()>) -> Result<Vec<LeadEvent>, String> {
    let LeadEvent::Review { plan_id, verdict, .. } = review else { return Err("검토 결과가 아닙니다".into()) };
    let thread = plan_thread(plan_id).ok_or("계획 id 형식이 아닙니다")?;
    let _locks = lock_latest(home, plan_id)?;
    append(home, thread, review).map_err(|e| e.to_string())?;
    let mut added = vec![review.clone()];
    if *verdict != prompt::Verdict::Approve { return Ok(added); }
    let (plan, ok) = read_thread(home, thread).into_iter().find_map(|e| match e {
        LeadEvent::Plan { plan_id: id, plan, ok, .. } if id == *plan_id => Some((plan, ok)),
        _ => None,
    }).ok_or("이 대화에 그런 계획이 없습니다")?;
    let mode = RunMode { yolo: yolo::read_enabled(home), ..RunMode::of(profile) };
    added.extend(auto_dispatch(home, plan_id, &plan, ok, mode, since, None, start, false)?);
    Ok(added)
}

/// Runs a plan without 실행, under [`lock_latest`]'s locks, unless something says to wait: a stop
/// of its thread since `since`, `wait` (why it may not run by itself), the policy, or the
/// auto-run limits (ADR-0014). Every outcome is a `lead.autorun` in the plan's thread.
#[allow(clippy::too_many_arguments)]
fn auto_dispatch(home: &Path, plan_id: &str, plan: &Value, ok: bool, mode: RunMode, since: DateTime<Utc>, wait: Option<String>, start: &dyn Fn(&str) -> io::Result<()>, scheduled: bool) -> Result<Vec<LeadEvent>, String> {
    let thread = plan_thread(plan_id).ok_or("계획 id 형식이 아닙니다")?;
    let plan_id = plan_id.to_string();
    let mut added = Vec::new();
    let events = read_thread(home, thread);
    let now = Utc::now();
    let checked = policy_context(home, now).map_err(|e| e.to_string()).map(|ctx| check_plan_in(plan, &ctx, mode));
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
        .and_then(|at| chrono::DateTime::parse_from_rfc3339(&at).ok()).is_some_and(|at| at.with_timezone(&Utc) >= since);
    let reason = if stopped { Some("모두 중지 요청으로 자동 실행을 취소했습니다".to_string()) }
        else if wait.is_some() { wait }
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
        match dispatch_locked(home, &plan_id, mode, start, false, scheduled) {
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
