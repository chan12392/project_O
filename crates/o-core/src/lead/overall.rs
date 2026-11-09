//! The overall lead (ADR-0032, ADR-0036): the lead thread of the projects root. It sees every
//! listed project's state, designs (```` ```o-design ````) and proposes requests for project leads
//! (```` ```o-forward ````), which go on when the person presses 요청 전달. It reviews the plans
//! they make of them (```` ```o-review ````); an approved plan runs. It never proposes cards
//! itself. What it may pass on, and which plan it reviews, is fixed here, not left to the model.
//!
//! ```text
//! leads/<threadId>/forward-plans/<proposalId>   made once, when the person sends a proposal on
//! leads/<threadId>/forwarded/<n>                made once, when `o _lead-forward` takes the thread's n-th lead.forward
//! ```

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;

use crate::agents::clean;
use crate::policy::LIMITS;
use crate::projects;
use crate::run::mode::Profile;
use crate::run::runner::Spawner;
use crate::run::snapshot::snapshot;
use crate::run::store;
use crate::run::yolo;
use crate::sys::js::{js_head, js_len, now_iso};

use super::prompt::{self, Forward, Verdict};
use super::{append, current_thread, open_threads, read_thread, running, thread_dir, thread_project, LeadEvent, SendRequest};

/// Requests one turn may pass on, one per project.
pub const MAX_FORWARDS: usize = 3;
/// Revisions one proposal may ask of one project before its plan waits for the person.
pub const MAX_REVISIONS: usize = 2;
/// What a review's data (request, design, plan) may take of the prompt.
const REVIEW_CHARS: usize = 12_000;
/// What the overview may take of the prompt, and of it each project's lines.
const OVERVIEW_CHARS: usize = 8_000;
const SNIPPET_CHARS: usize = 200;
/// Projects shown, and runs of each.
const MAX_PROJECTS: usize = 40;
const RUNS_PER_PROJECT: usize = 3;

/// The projects root the overall lead's thread is filed under, when it exists.
pub fn root(home: &Path) -> Option<String> {
    let listing = projects::list(home, &store::user_home()).ok()?;
    listing.root_exists.then_some(listing.root)
}

/// Whether `project` is the overall lead's folder (the projects root).
pub fn is_overall(home: &Path, project: &str) -> bool {
    root(home).is_some_and(|root| projects::same(&root, project))
}

fn clip(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let line = if js_len(&line) > SNIPPET_CHARS { format!("{}…", js_head(&line, SNIPPET_CHARS)) } else { line };
    serde_json::to_string(&line).unwrap_or_default()
}

/// Every listed project's open-thread count and current lead's last request and answer, a plan not
/// yet run, a question waiting for the person, and its latest runs. Untrusted data for the prompt.
pub fn overview(home: &Path) -> String {
    let Ok(listing) = projects::list(home, &store::user_home()) else { return "프로젝트 목록을 읽지 못했습니다.".into() };
    let mut out = Vec::new();
    let mut size = 0;
    let shown: Vec<_> = listing.projects.iter().filter(|p| !p.missing).take(MAX_PROJECTS).collect();
    for project in &shown {
        let mut lines = vec![format!("- {} {}", project.name, serde_json::to_string(&project.path).unwrap_or_default())];
        lines.push(format!("  - 열린 총괄 대화: {}개 (아래는 현재 탭 기준)", open_threads(home, &project.path).len()));
        match current_thread(home, &project.path) {
            None => lines.push("  - 총괄 대화 없음".into()),
            Some(id) => {
                let events = read_thread(home, &id);
                if running(home, &id).is_some() { lines.push("  - 총괄이 답하는 중".into()); }
                let last_user = events.iter().rposition(|e| matches!(e, LeadEvent::User { .. }));
                if let Some(LeadEvent::User { text, origin, .. }) = last_user.map(|i| &events[i]) {
                    lines.push(format!("  - 마지막 요청({}): {}", if origin.is_some() { "전체 총괄" } else { "사람" }, clip(text)));
                }
                if let Some(text) = events.iter().rev().find_map(|e| match e { LeadEvent::Reply { text, .. } => Some(text), _ => None }) {
                    lines.push(format!("  - 마지막 답: {}", clip(text)));
                }
                if last_user.is_some_and(|i| events[i..].iter().any(|e| matches!(e, LeadEvent::Ask { .. }))) {
                    lines.push("  - 사람의 답을 기다리는 질문이 있음".into());
                }
                let dispatched: BTreeSet<_> = events.iter().filter_map(|e| match e { LeadEvent::Dispatch { plan_id, .. } => Some(plan_id.as_str()), _ => None }).collect();
                if let Some((title, ok)) = events.iter().rev().find_map(|e| match e { LeadEvent::Plan { plan_id, plan, ok, .. } => Some((plan_id, plan, ok)), _ => None })
                    .filter(|(id, _, _)| !dispatched.contains(id.as_str())).map(|(_, plan, ok)| (plan["title"].as_str().unwrap_or_default().to_string(), *ok)) {
                    lines.push(format!("  - 실행하지 않은 계획: {} ({})", clip(&title), if ok { "사람의 실행을 기다림" } else { "정책 차단" }));
                }
                let runs: Vec<_> = events.iter().rev().filter_map(|e| match e { LeadEvent::Dispatch { run_id, .. } | LeadEvent::Followup { run_id, .. } => Some(run_id), _ => None })
                    .filter(|id| store::is_run_id(id)).take(RUNS_PER_PROJECT).collect();
                for run in runs.into_iter().rev() {
                    match snapshot(&store::read_events(home, run)) {
                        Some(view) => lines.push(format!("  {}", prompt::run_status(&view).lines().next().unwrap_or_default())),
                        None => lines.push(format!("  - 실행 {run}: 상태 불명")),
                    }
                }
            }
        }
        let block = lines.join("\n");
        if size + js_len(&block) > OVERVIEW_CHARS {
            out.push(format!("(나머지 {}개 프로젝트 생략)", shown.len() - out.len()));
            break;
        }
        size += js_len(&block) + 1;
        out.push(block);
    }
    if out.is_empty() { return "등록된 프로젝트가 없습니다.".into(); }
    out.join("\n")
}

/// The requests a turn may pass on: listed projects other than the root, each once, at most
/// [`MAX_FORWARDS`], with text a card's task could hold. The paths come back as the list has them.
pub fn check(home: &Path, items: Vec<Forward>) -> Result<Vec<Forward>, String> {
    if items.is_empty() || items.len() > MAX_FORWARDS {
        return Err(format!("한 번에 넘길 수 있는 요청은 1~{MAX_FORWARDS}개입니다"));
    }
    let listing = projects::list(home, &store::user_home())?;
    let mut checked: Vec<Forward> = Vec::new();
    for item in items {
        let Some(listed) = listing.projects.iter().find(|p| !p.missing && projects::same(&p.path, &item.project)) else {
            return Err(format!("목록에 없는 프로젝트로는 넘기지 않습니다: {}", item.project));
        };
        if projects::same(&listed.path, &listing.root) { return Err("전체 총괄 자신에게는 넘기지 않습니다".into()); }
        if checked.iter().any(|c| projects::same(&c.project, &listed.path)) { return Err(format!("한 프로젝트에는 요청을 하나만 넘깁니다: {}", listed.name)); }
        let text = item.text.trim();
        if text.is_empty() { return Err(format!("{}에 넘길 요청이 비어 있습니다", listed.name)); }
        if js_len(text) > LIMITS.max_task_chars { return Err(format!("{}에 넘길 요청은 {}자 이내입니다", listed.name, LIMITS.max_task_chars)); }
        checked.push(Forward { project: listed.path.clone(), text: text.to_string() });
    }
    Ok(checked)
}

/// `<threadId>-f<n>` → the thread id.
fn proposal_thread(proposal_id: &str) -> Option<&str> {
    let (thread, n) = proposal_id.rsplit_once("-f")?;
    (store::is_run_id(thread) && !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())).then_some(thread)
}

fn project_name(project: &str) -> String {
    Path::new(project).file_name().map_or_else(|| project.to_string(), |n| n.to_string_lossy().into_owned())
}

/// Starts `o _lead-forward` (through `start`, with the thread and the request's number) for each
/// request; a start that fails is noted in the thread.
fn start_forwards(home: &Path, thread_id: &str, indexes: impl Iterator<Item = usize>, start: &dyn Fn(&str, usize) -> io::Result<()>) {
    for index in indexes {
        if let Err(error) = start(thread_id, index) {
            let _ = append(home, thread_id, &LeadEvent::Warning { at: now_iso(), text: format!("요청 전달을 시작하지 못했습니다: {error}") });
        }
    }
}

/// `o lead forward <proposalId>` (ADR-0036): the person sends the overall lead's proposed requests
/// on, once, while the proposal is its open thread's latest word and no turn answers there. The
/// requests are checked again and recorded as `lead.forward`s, then go on through `start`.
pub fn send_on(home: &Path, proposal_id: &str, start: &dyn Fn(&str, usize) -> io::Result<()>) -> Result<Vec<LeadEvent>, String> {
    let thread_id = proposal_thread(proposal_id).ok_or_else(|| format!("요청 제안 id 형식이 아닙니다: {proposal_id}"))?;
    let (first, added) = {
        // The dispatch order: the thread list, the thread's turn, its dispatch.
        let _current = super::tabs::current_lock(home).map_err(|e| e.to_string())?;
        let turn = super::turn_lock(home, thread_id).map_err(|e| e.to_string())?;
        turn.try_lock().map_err(|_| "전체 총괄이 답하는 중입니다. 답을 확인한 뒤 전달하세요")?;
        if super::running_path(home, thread_id).exists() { return Err(super::UNKNOWN.into()); }
        let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(thread_dir(home, thread_id).join("dispatch.lock")).map_err(|e| e.to_string())?;
        lock.lock().map_err(|e| e.to_string())?;
        let events = read_thread(home, thread_id);
        let project = thread_project(&events).ok_or("총괄 대화를 찾지 못했습니다")?;
        if !is_overall(home, &project) { return Err("전체 총괄 대화가 아닙니다".into()); }
        let latest = events.iter().rev().find_map(|event| match event {
            LeadEvent::ForwardPlan { proposal_id, .. } => Some(Some(proposal_id.as_str())),
            LeadEvent::User { .. } => Some(None),
            _ => None,
        }).flatten();
        if !super::is_open(home, &project, thread_id) || latest != Some(proposal_id) {
            return Err("제안이 바뀌었습니다. 최신 제안을 확인한 뒤 전달하세요".into());
        }
        let Some(LeadEvent::ForwardPlan { items, provider, model, effort, profile, .. }) =
            events.iter().find(|e| matches!(e, LeadEvent::ForwardPlan { proposal_id: id, .. } if id == proposal_id)).cloned() else { return Err("그런 제안이 없습니다".into()) };
        let items = check(home, items)?;
        let marks = thread_dir(home, thread_id).join("forward-plans");
        fs::create_dir_all(&marks).map_err(|e| e.to_string())?;
        OpenOptions::new().write(true).create_new(true).open(marks.join(proposal_id)).map_err(|_| "이미 전달한 제안입니다".to_string())?;
        let first = events.iter().filter(|e| matches!(e, LeadEvent::Forward { .. })).count();
        let mut added = Vec::new();
        for item in items {
            let event = LeadEvent::Forward {
                at: now_iso(), project: item.project, text: item.text, provider, model: model.clone(), effort: effort.clone(), profile,
                proposal: Some(proposal_id.to_string()), revision: None,
            };
            append(home, thread_id, &event).map_err(|e| e.to_string())?;
            added.push(event);
        }
        (first, added)
    };
    start_forwards(home, thread_id, first..first + added.len(), start);
    Ok(added)
}

/// How `o _lead-forward` starts what follows a request: the runs an approved plan makes
/// (`o _run <runId>`), and a revision request passed on again (`o _lead-forward <threadId> <n>`).
pub struct Starts<'a> {
    pub run: &'a dyn Fn(&str) -> io::Result<()>,
    pub forward: &'a dyn Fn(&str, usize) -> io::Result<()>,
}

/// A plan O asks the overall lead to review (ADR-0036): which one, and what the prompt shows of it.
pub struct ReviewTarget {
    pub project: String,
    pub plan_id: String,
    /// The request, the project lead's design and the plan, as untrusted data.
    pub data: String,
}

/// `o _lead-forward <threadId> <n>`: sends the overall thread's n-th request (0 is the first)
/// to its project's lead, once, in the mode set now (`o yolo`). Nobody is there to approve
/// actions, so a normal-mode turn is read-only like a scheduled one. A request the person sent
/// on (ADR-0036) has its plan reviewed by the overall lead next; any other plan waits for the
/// person's "실행". A failure is noted in the overall thread.
pub fn forward(home: &Path, thread_id: &str, index: usize, spawner: &Spawner, starts: &Starts) -> Result<Vec<LeadEvent>, String> {
    if !store::is_run_id(thread_id) { return Err("대화 id가 올바르지 않습니다".into()); }
    let events = read_thread(home, thread_id);
    if !thread_project(&events).is_some_and(|project| is_overall(home, &project)) { return Err("전체 총괄 대화가 아닙니다".into()); }
    let Some(request) = events.iter().filter(|e| matches!(e, LeadEvent::Forward { .. })).nth(index).cloned() else { return Err("넘길 요청이 없습니다".into()) };
    let LeadEvent::Forward { at, project, text, provider, model, effort, profile, proposal, revision } = &request else { unreachable!() };
    let root = (*profile == Profile::ProjectO).then(clean::default_root);
    // A revision goes back to the thread whose plan it revises, while that thread is open.
    let destination = match revision.as_deref().and_then(super::plan_thread) {
        Some(thread) if super::is_open(home, project, thread) => Ok(thread.to_string()),
        Some(_) => Err("계획을 낸 총괄 대화가 닫혔습니다".to_string()),
        None => super::select_thread(home, project, None, false),
    };
    let sent = destination.and_then(|destination| {
        let req = SendRequest {
            project,
            thread: Some(&destination),
            provider: *provider,
            model: model.as_deref(),
            effort: effort.as_deref(),
            text,
            attach: &[],
            fresh: false,
            clean: root.as_deref(),
            yolo: yolo::read_enabled(home),
        };
        super::send_forwarded(home, &req, spawner, thread_id, at, index)
    });
    let warn = |text: String| { let _ = append(home, thread_id, &LeadEvent::Warning { at: now_iso(), text }); };
    let name = project_name(project);
    let added = match sent {
        Err(error) => { warn(format!("{name}에 요청을 넘기지 못했습니다: {error}")); return Err(error); }
        Ok(added) => added,
    };
    let Some(proposal) = proposal else { return Ok(added) };
    let Some((plan_id, ok)) = added.iter().find_map(|e| match e { LeadEvent::Plan { plan_id, ok, .. } => Some((plan_id.clone(), *ok)), _ => None }) else { return Ok(added) };
    if !ok {
        warn(format!("{name}의 계획이 정책 검사에 걸려 검토하지 않았습니다. 프로젝트에서 확인하세요"));
        return Ok(added);
    }
    if let Err(error) = review(home, thread_id, &request, proposal, &plan_id, spawner, starts) {
        warn(format!("{name}의 계획을 검토하지 못했습니다: {error}. 계획은 사람의 실행을 기다립니다"));
    }
    Ok(added)
}

/// The overall lead reviews `plan_id`, made of `request`, in its thread `source`, with the
/// request's agent and profile, once no other turn answers there. Its verdict lands in both
/// threads; `approve` runs the plan, `revise` passes the notes on to the project lead again.
fn review(home: &Path, source: &str, request: &LeadEvent, proposal: &str, plan_id: &str, spawner: &Spawner, starts: &Starts) -> Result<(), String> {
    let LeadEvent::Forward { at, project, text, provider, model, effort, profile, .. } = request else { return Err("전달 기록이 아닙니다".into()) };
    let root_project = thread_project(&read_thread(home, source)).ok_or("전체 총괄 대화를 확인할 수 없습니다")?;
    let deadline = Instant::now() + super::TURN_LIMIT;
    while running(home, source).is_some() {
        if Instant::now() > deadline { return Err("전체 총괄이 오래 답하는 중입니다".into()); }
        thread::sleep(Duration::from_secs(1));
    }
    let since = chrono::DateTime::parse_from_rfc3339(at).map_err(|e| e.to_string())?.with_timezone(&Utc);
    let target = ReviewTarget { project: project.clone(), plan_id: plan_id.to_string(), data: review_data(home, plan_id, text)? };
    let ask = format!("{}의 계획 {plan_id}을 검토해 주세요", project_name(project));
    let root = (*profile == Profile::ProjectO).then(clean::default_root);
    let req = SendRequest {
        project: &root_project,
        thread: Some(source),
        provider: *provider,
        model: model.as_deref(),
        effort: effort.as_deref(),
        text: &ask,
        attach: &[],
        fresh: false,
        clean: root.as_deref(),
        yolo: false,
    };
    if stopped_since(home, source, since) { return Err("전체 총괄이 중지되었습니다".into()); }
    let turn_started = Utc::now();
    let added = super::send_review(home, &req, spawner, &target)?;
    // No verdict: the turn noted why (no block, other blocks beside it, a question).
    let Some(verdict) = added.iter().find(|e| matches!(e, LeadEvent::Review { .. })).cloned() else { return Ok(()) };
    let LeadEvent::Review { verdict: word, notes, .. } = &verdict else { unreachable!() };
    {
        let _current = super::tabs::current_lock(home).map_err(|e| e.to_string())?;
        if stopped_since(home, source, turn_started) || !super::is_open(home, &root_project, source) {
            return Err("전체 총괄이 중지되었거나 닫혀 검토 결과를 적용하지 않았습니다".into());
        }
    }
    let applied = super::apply_review(home, &verdict, *profile, since, starts.run).map_err(|why| format!("검토 결과를 적용하지 못했습니다({why})"))?;
    match word {
        Verdict::Approve => {
            if !applied.iter().any(|e| matches!(e, LeadEvent::Dispatch { .. })) {
                let why = applied.iter().rev().find_map(|e| match e { LeadEvent::AutoRun { reason: Some(reason), .. } => Some(reason.as_str()), _ => None }).unwrap_or("알 수 없음");
                append(home, source, &LeadEvent::Warning { at: now_iso(), text: format!("승인한 계획을 실행하지 않았습니다: {why}") }).map_err(|e| e.to_string())?;
            }
        }
        Verdict::Revise => revise(home, source, request, proposal, plan_id, notes, starts.forward)?,
    }
    Ok(())
}

/// Passes a review's notes on to the project lead that made `plan_id`, at most [`MAX_REVISIONS`]
/// times for one proposal and project.
fn revise(home: &Path, source: &str, request: &LeadEvent, proposal: &str, plan_id: &str, notes: &str, start: &dyn Fn(&str, usize) -> io::Result<()>) -> Result<(), String> {
    let LeadEvent::Forward { project, provider, model, effort, profile, .. } = request else { return Err("전달 기록이 아닙니다".into()) };
    let index = {
        let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(thread_dir(home, source).join("dispatch.lock")).map_err(|e| e.to_string())?;
        lock.lock().map_err(|e| e.to_string())?;
        let events = read_thread(home, source);
        let revisions = events.iter().filter(|e| matches!(e, LeadEvent::Forward { proposal: Some(p), revision: Some(_), project: other, .. }
            if p == proposal && projects::same(other, project))).count();
        let notes = notes.trim();
        let refused = if revisions >= MAX_REVISIONS { Some(format!("보완 요청이 {MAX_REVISIONS}번을 넘어 더 넘기지 않습니다")) }
            else if notes.is_empty() { Some("보완할 내용이 비어 있어 넘기지 않습니다".to_string()) }
            else if js_len(notes) > LIMITS.max_task_chars { Some(format!("보완 요청은 {}자 이내여야 합니다", LIMITS.max_task_chars)) }
            else { None };
        if let Some(why) = refused {
            return append(home, source, &LeadEvent::Warning { at: now_iso(), text: format!("{}: {why}. 계획은 사람의 실행을 기다립니다", project_name(project)) }).map_err(|e| e.to_string());
        }
        append(home, source, &LeadEvent::Forward {
            at: now_iso(), project: project.clone(), text: notes.to_string(), provider: *provider, model: model.clone(), effort: effort.clone(), profile: *profile,
            proposal: Some(proposal.to_string()), revision: Some(plan_id.to_string()),
        }).map_err(|e| e.to_string())?;
        events.iter().filter(|e| matches!(e, LeadEvent::Forward { .. })).count()
    };
    start_forwards(home, source, std::iter::once(index), start);
    Ok(())
}

/// Whether the thread was stopped (모두 중지) at or after `since`.
fn stopped_since(home: &Path, thread_id: &str, since: chrono::DateTime<Utc>) -> bool {
    fs::read_to_string(thread_dir(home, thread_id).join("stop-all-at")).ok()
        .and_then(|at| chrono::DateTime::parse_from_rfc3339(&at).ok()).is_some_and(|at| at.with_timezone(&Utc) >= since)
}

/// What the review prompt shows: the request, the project lead's latest design and the plan,
/// clipped to [`REVIEW_CHARS`].
fn review_data(home: &Path, plan_id: &str, request: &str) -> Result<String, String> {
    let thread = super::plan_thread(plan_id).ok_or("계획 id 형식이 아닙니다")?;
    let events = read_thread(home, thread);
    let plan = events.iter().find_map(|e| match e { LeadEvent::Plan { plan_id: id, plan, .. } if id == plan_id => Some(plan), _ => None }).ok_or("계획을 찾지 못했습니다")?;
    let design = events.iter().rev().find_map(|e| match e { LeadEvent::Design { design, .. } => Some(design.to_string()), _ => None });
    let data = format!(
        "넘긴 요청: {}\n프로젝트 총괄의 설계: {}\n계획 {plan_id}: {plan}",
        serde_json::to_string(request).unwrap_or_default(),
        design.as_deref().unwrap_or("없음"),
    );
    Ok(if js_len(&data) > REVIEW_CHARS { format!("{}…(생략)", js_head(&data, REVIEW_CHARS)) } else { data })
}
