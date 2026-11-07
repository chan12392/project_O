//! The overall lead (ADR-0032): the lead thread of the projects root. It sees every listed
//! project's state and passes requests on to project leads (```` ```o-forward ````); it never
//! proposes cards itself. What it may pass on is fixed here, not left to the model.
//!
//! ```text
//! leads/<threadId>/forwarded/<n>   made once, when `o _lead-forward` takes the thread's n-th lead.forward
//! ```

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::path::Path;

use crate::agents::clean;
use crate::policy::LIMITS;
use crate::projects;
use crate::run::mode::Profile;
use crate::run::runner::Spawner;
use crate::run::snapshot::snapshot;
use crate::run::store;
use crate::run::yolo;
use crate::sys::js::{js_head, js_len, now_iso};

use super::prompt::{self, Forward};
use super::{append, current_thread, read_thread, running, thread_dir, thread_project, LeadEvent, SendRequest};

/// Requests one turn may pass on, one per project.
pub const MAX_FORWARDS: usize = 3;
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

/// Every listed project's state as O knows it: its lead's last request and answer, a plan not
/// yet run, a question waiting for the person, and its latest runs. Untrusted data for the prompt.
pub fn overview(home: &Path) -> String {
    let Ok(listing) = projects::list(home, &store::user_home()) else { return "프로젝트 목록을 읽지 못했습니다.".into() };
    let mut out = Vec::new();
    let mut size = 0;
    let shown: Vec<_> = listing.projects.iter().filter(|p| !p.missing).take(MAX_PROJECTS).collect();
    for project in &shown {
        let mut lines = vec![format!("- {} {}", project.name, serde_json::to_string(&project.path).unwrap_or_default())];
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

/// After an overall turn: starts `o _lead-forward` (through `start`, with the thread and the
/// request's number) for each request the turn `added`. A start that fails is noted in the thread.
pub fn pass_on(home: &Path, thread_id: &str, added: &[LeadEvent], start: &dyn Fn(&str, usize) -> std::io::Result<()>) {
    let new = added.iter().filter(|e| matches!(e, LeadEvent::Forward { .. })).count();
    if new == 0 { return; }
    let all = read_thread(home, thread_id).iter().filter(|e| matches!(e, LeadEvent::Forward { .. })).count();
    for index in all.saturating_sub(new)..all {
        if let Err(error) = start(thread_id, index) {
            let _ = append(home, thread_id, &LeadEvent::Warning { at: now_iso(), text: format!("요청 전달을 시작하지 못했습니다: {error}") });
        }
    }
}

/// `o _lead-forward <threadId> <n>`: sends the overall thread's n-th request (0 is the first)
/// to its project's lead, once, in the mode set now (`o yolo`). Nobody is there to approve
/// actions, so a normal-mode turn is read-only like a scheduled one; its plan waits for the
/// person's "실행". A failure is noted in the overall thread.
pub fn forward(home: &Path, thread_id: &str, index: usize, spawner: &Spawner) -> Result<Vec<LeadEvent>, String> {
    if !store::is_run_id(thread_id) { return Err("대화 id가 올바르지 않습니다".into()); }
    let events = read_thread(home, thread_id);
    if !thread_project(&events).is_some_and(|project| is_overall(home, &project)) { return Err("전체 총괄 대화가 아닙니다".into()); }
    let Some(LeadEvent::Forward { project, text, provider, model, effort, profile, .. }) =
        events.iter().filter(|e| matches!(e, LeadEvent::Forward { .. })).nth(index).cloned() else { return Err("넘길 요청이 없습니다".into()) };
    let marks = thread_dir(home, thread_id).join("forwarded");
    fs::create_dir_all(&marks).map_err(|e| e.to_string())?;
    OpenOptions::new().write(true).create_new(true).open(marks.join(index.to_string())).map_err(|_| "이미 넘긴 요청입니다".to_string())?;
    let root = (profile == Profile::ProjectO).then(clean::default_root);
    let req = SendRequest {
        project: &project,
        provider,
        model: model.as_deref(),
        effort: effort.as_deref(),
        text: &text,
        attach: &[],
        fresh: false,
        clean: root.as_deref(),
        yolo: yolo::read_enabled(home),
    };
    let sent = super::send_forwarded(home, &req, spawner);
    if let Err(error) = &sent {
        let name = Path::new(&project).file_name().map_or_else(|| project.clone(), |n| n.to_string_lossy().into_owned());
        let _ = append(home, thread_id, &LeadEvent::Warning { at: now_iso(), text: format!("{name}에 요청을 넘기지 못했습니다: {error}") });
    }
    sent
}
