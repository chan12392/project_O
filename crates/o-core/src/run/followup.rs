//! Resolve a completed card to the session and workspace that can be continued.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::run::events::{CardInfo, CardStatus, OEvent};
use crate::run::plan::{Card, Plan, ProviderId};
use crate::run::mode::{Profile, RunMode};
use crate::run::store::{self, is_clean, is_run_id, read_events, read_plan, run_dir, run_is_live};

pub struct Target {
    pub card: Card,
    pub project: String,
    pub workspace: PathBuf,
    pub session: String,
    pub clean: bool,
}

/// Uses only O's saved run, never a caller-provided executable, session or workspace.
pub fn target(home: &Path, run: &str, card: &str) -> Result<Target, String> {
    target_with_depth(home, run, card, 0)
}

fn target_with_depth(home: &Path, run: &str, card: &str, depth: usize) -> Result<Target, String> {
    if depth > 32 { return Err("후속 작업 연결이 너무 깊습니다".into()); }
    if !is_run_id(run) { return Err("실행 ID가 올바르지 않습니다".into()); }
    let dir = run_dir(home, run);
    let plan: Plan = serde_json::from_value(read_plan(home, run).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let index = plan.cards.iter().position(|item| item.id == card).ok_or("해당 작업을 찾지 못했습니다")?;
    let card = plan.cards[index].clone();
    if card.provider == ProviderId::Agy { return Err(crate::policy::AGY_UNAVAILABLE.into()); }
    let events = read_events(home, run);
    let ended = events.iter().rev().find_map(|event| match event {
        OEvent::CardEnd { card_id, status, .. } if card_id == &card.id => Some(*status),
        _ => None,
    });
    let active = store::active_markers(home);
    if active.iter().any(|m| m.run_id == run && m.card_id == card.id)
        || (ended.is_none() && (run_is_live(home, run) || !dir.join("followup.json").exists() || active.iter().any(|m| m.run_id == run))) {
        return Err("이전 작업이 아직 실행 중이거나 정상적으로 종료되지 않았습니다".into());
    }
    let mut session = events.iter().rev().find_map(|event| match event {
        OEvent::CardSession { card_id, session_id, .. } if card_id == &card.id => Some(session_id.clone()),
        _ => None,
    });
    let workspace = if dir.join("followup.json").exists() {
        let reference: (String, String) = serde_json::from_slice(&fs::read(dir.join("followup.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let previous = target_with_depth(home, &reference.0, &reference.1, depth + 1)?;
        if previous.card.id != card.id || previous.card.provider != card.provider || previous.card.model != card.model || previous.project != plan.project || previous.clean != is_clean(home, run) {
            return Err("기존 후속 작업 정보가 바뀌었습니다".into());
        }
        if session.is_none() { session = Some(previous.session); }
        previous.workspace
    } else if dir.join("isolated-worktrees").exists() {
        let name = home.file_name().ok_or("상태 폴더가 올바르지 않습니다")?.to_string_lossy();
        let base = home.with_file_name(format!("{name}-worktrees"));
        let run_root = base.join(run);
        let expected = run_root.join((index + 1).to_string());
        for path in [&base, &run_root, &expected] {
            let metadata = fs::symlink_metadata(path).map_err(|_| "기존 작업 폴더를 찾지 못했습니다")?;
            if metadata.file_type().is_symlink() { return Err("기존 작업 폴더가 링크로 변경되었습니다".into()); }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 { return Err("기존 작업 폴더가 연결 지점으로 변경되었습니다".into()); }
            }
        }
        let root = resolved(&expected, "기존 작업 폴더를 찾지 못했습니다")?;
        let path = events.iter().find_map(|event| match event {
            OEvent::CardTool { card_id, name, summary, .. } if card_id == &card.id && name == "O 작업 폴더" => Some(summary),
            _ => None,
        }).ok_or("기존 작업 폴더 기록이 없습니다")?;
        let actual = resolved(Path::new(path), "기존 작업 폴더를 찾지 못했습니다")?;
        if !actual.starts_with(&root) || !actual.is_dir() { return Err("기존 작업 폴더가 변경되었습니다".into()); }
        actual
    } else {
        resolved(Path::new(&plan.project), "기존 프로젝트를 찾지 못했습니다")?
    };
    let session = session.ok_or("이어갈 세션 ID가 없습니다")?;
    if !is_run_id(&session) { return Err("세션 ID가 올바르지 않습니다".into()); }
    Ok(Target { card, project: plan.project, workspace, session, clean: is_clean(home, run) })
}

/// The existing folder a path names, links resolved, in the plain form the rest of O uses
/// (`store::resolved_path`: no extended-length prefix on Windows), so a resumed card's cwd,
/// plan project and Codex `-C` match the folders the first run used.
fn resolved(path: &Path, missing: &str) -> Result<PathBuf, String> {
    path.canonicalize().map_err(|_| missing.to_string())?;
    Ok(PathBuf::from(store::resolved_path(&path.to_string_lossy())))
}

/// Creates a policy-checked run that resumes the original card in its own workspace.
pub fn start(home: &Path, run: &str, card: &str, text: &str, spawn: &dyn Fn(&str) -> io::Result<()>) -> Result<String, String> {
    if !is_run_id(run) { return Err("실행 ID가 올바르지 않습니다".into()); }
    let source: Plan = serde_json::from_value(read_plan(home, run).map_err(|_| "이 작업의 실행 정보를 찾지 못했습니다")?).map_err(|_| "이 작업의 실행 정보가 올바르지 않습니다")?;
    let mut next_card = source.cards.into_iter().find(|item| item.id == card).ok_or("해당 작업을 찾지 못했습니다")?;
    if next_card.provider == ProviderId::Agy { return Err(crate::policy::AGY_UNAVAILABLE.into()); }
    let running = run_is_live(home, run) && !read_events(home, run).iter().any(|event| matches!(event, OEvent::CardEnd { card_id, .. } if card_id == card));
    if !running { target(home, run, card)?; }
    next_card.task = text.to_string();
    next_card.depends_on.clear();
    let plan = Plan { title: "후속 작업".into(), project: source.project, cards: vec![next_card] };
    let raw = serde_json::to_value(&plan).map_err(|e| e.to_string())?;
    let mut ctx = store::policy_context(home, Utc::now()).map_err(|e| e.to_string())?;
    // Queuing takes no execution slot; real active caps are checked again at launch.
    ctx.active_by_provider.clear();
    let profile = Profile::of_clean(is_clean(home, run));
    let checked = crate::policy::check_plan_for(&raw, &ctx, profile);
    let plan = checked.plan.ok_or_else(|| checked.violations.iter().map(|v| v.message.as_str()).collect::<Vec<_>>().join(" / "))?;
    let id = store::new_run_id(Utc::now());
    store::create_run(home, &id, &plan).map_err(|e| e.to_string())?;
    store::mark_mode(home, &id, &RunMode { profile, context: true, yolo: false }).map_err(|e| e.to_string())?;
    fs::write(run_dir(home, &id).join("followup.json"), serde_json::to_vec(&(run, card)).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let item = &plan.cards[0];
    let at = crate::sys::js::now_iso();
    for event in [
        OEvent::RunStart { at: at.clone(), run_id: id.clone(), title: plan.title.clone(), project: plan.project.clone(), cards: vec![CardInfo { id: item.id.clone(), provider: item.provider, label: Some(item.label()), role: item.role.clone(), model: item.model.clone(), effort: item.effort.clone() }] },
        OEvent::RunFollowup { at: at.clone(), source_run_id: run.into(), card_id: card.into() },
        OEvent::RunMode { at: at.clone(), run_id: id.clone(), profile, context: true, yolo: false },
        OEvent::CardInput { at, card_id: card.into(), text: item.task.clone() },
    ] { store::append_event(home, &id, &event).map_err(|e| e.to_string())?; }
    if let Err(error) = spawn(&id) {
        let card = &plan.cards[0];
        let at = crate::sys::js::now_iso();
        let _ = store::append_event(home, &id, &OEvent::CardEnd { at: at.clone(), card_id: card.id.clone(), status: CardStatus::Failed, result: Some(format!("실행기를 시작하지 못했습니다: {error}")), exit_code: None });
        let _ = store::append_event(home, &id, &OEvent::RunEnd { at, run_id: id.clone(), status: CardStatus::Failed });
        return Err(format!("후속 실행을 시작하지 못했습니다: {error}"));
    }
    Ok(id)
}
