//! Resolve a completed card to the session and workspace that can be continued.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::run::events::{CardInfo, CardStatus, OEvent};
use crate::run::plan::{Card, Plan};
use crate::run::mode::{Profile, RunMode};
use crate::run::store::{self, is_clean, is_run_id, read_events, read_plan, run_dir, run_is_live};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub card: Card,
    pub project: String,
    pub workspace: PathBuf,
    /// The agent session to resume; `None` starts a fresh one in the same workspace, after a card
    /// whose runner died closed it mid-session (ADR-0039).
    pub session: Option<String>,
    pub clean: bool,
}

/// Marks a follow-up accepted while its original card was still running.
pub(crate) const QUEUED_BEHIND: &str = "followup-queued-behind";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Source {
    run: String,
    plan: Plan,
    clean: bool,
    isolated: bool,
    previous: Option<(String, String)>,
}

impl Source {
    fn read(home: &Path, run: &str) -> Result<Self, String> {
        if !is_run_id(run) { return Err("실행 ID가 올바르지 않습니다".into()); }
        let dir = run_dir(home, run);
        let previous = match fs::read(dir.join("followup.json")) {
            Ok(bytes) => Some(serde_json::from_slice(&bytes).map_err(|e| e.to_string())?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.to_string()),
        };
        Ok(Self { run: run.into(), plan: serde_json::from_value(read_plan(home, run).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?, clean: is_clean(home, run), isolated: dir.join("isolated-worktrees").exists(), previous })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Input {
    pub source: String,
    pub card: String,
    pub root: String,
    pub queued_behind: bool,
    sources: Vec<Source>,
    completed: Option<Target>,
}

impl Input {
    fn capture(home: &Path, run: &str, card: &str, running: bool) -> Result<Self, String> {
        let mut sources: Vec<Source> = Vec::new();
        let mut root = run.to_string();
        loop {
            if sources.len() >= 33 || sources.iter().any(|source| source.run == root) { return Err("후속 작업 연결이 너무 깊거나 순환합니다".into()); }
            let source = Source::read(home, &root)?;
            let previous = source.previous.clone();
            sources.push(source);
            match previous {
                Some((run, id)) if id == card => root = run,
                Some(_) => return Err("후속 작업 연결이 올바르지 않습니다".into()),
                None => break,
            }
        }
        let completed = if running { None } else { Some(target_with_depth(home, run, card, 0, Some(&sources))?) };
        Ok(Self { source: run.into(), card: card.into(), root, queued_behind: running, sources, completed })
    }

    pub fn target(&self, home: &Path) -> Result<Target, String> {
        match &self.completed {
            Some(target) => Ok(target.clone()),
            None => target_with_depth(home, &self.source, &self.card, 0, Some(&self.sources)),
        }
    }
}

/// Uses only O's saved run, never a caller-provided executable, session or workspace.
pub fn target(home: &Path, run: &str, card: &str) -> Result<Target, String> {
    target_with_depth(home, run, card, 0, None)
}

fn target_with_depth(home: &Path, run: &str, card: &str, depth: usize, sources: Option<&[Source]>) -> Result<Target, String> {
    if depth > 32 { return Err("후속 작업 연결이 너무 깊습니다".into()); }
    if !is_run_id(run) { return Err("실행 ID가 올바르지 않습니다".into()); }
    let source = match sources {
        Some(sources) => sources.iter().find(|source| source.run == run).cloned().ok_or("승인한 후속 작업 연결이 아닙니다")?,
        None => Source::read(home, run)?,
    };
    let plan = source.plan;
    let index = plan.cards.iter().position(|item| item.id == card).ok_or("해당 작업을 찾지 못했습니다")?;
    let requested = plan.cards[index].clone();
    let mut card = requested.clone();
    if crate::providers::get(card.provider).terminal_card() { return Err("터미널 카드는 후속 지시를 받지 않습니다".into()); }
    let events = read_events(home, run);
    let switched = events.iter().enumerate().rev().find_map(|(index, event)| match event {
        OEvent::CardFallback { card_id, target, .. } if card_id == &card.id => Some((index, target)),
        _ => None,
    });
    if let Some((_, target)) = switched { card.provider = target.provider; card.model = Some(target.model.clone()); }
    let last_end = events.iter().rev().find_map(|event| match event {
        OEvent::CardEnd { card_id, status, result, exit_code, .. } if card_id == &card.id => Some((*status, result.as_deref(), *exit_code)),
        _ => None,
    });
    let ended = last_end.map(|(status, _, _)| status);
    // A card the dead-runner close ended (ADR-0039) was cut off mid-session: never resume that session.
    let interrupted = last_end.is_some_and(|(status, result, exit_code)| store::closed_by_dead_runner(status, result, exit_code));
    let active = store::active_markers(home);
    if store::resource_held(home, run, &card.id).map_err(|e| e.to_string())?
        || active.iter().any(|m| m.run_id == run && m.card_id == card.id)
        || ended.is_none() {
        return Err("이전 작업이 아직 실행 중이거나 정상적으로 종료되지 않았습니다".into());
    }
    let mut session = events.iter().skip(switched.map_or(0, |(index, _)| index + 1)).rev().find_map(|event| match event {
        OEvent::CardSession { card_id, session_id, .. } if card_id == &card.id => Some(session_id.clone()),
        _ => None,
    }).filter(|_| !interrupted);
    // A follow-up of a fresh start (an interrupted card up the chain) that reported no session of
    // its own starts fresh too.
    let mut fresh = interrupted || (switched.is_some() && session.is_none());
    let workspace = if let Some(reference) = source.previous {
        let previous = target_with_depth(home, &reference.0, &reference.1, depth + 1, sources)?;
        if previous.card.id != requested.id || previous.card.provider != requested.provider || previous.card.model != requested.model || previous.project != plan.project || previous.clean != source.clean {
            return Err("기존 후속 작업 정보가 바뀌었습니다".into());
        }
        if session.is_none() && !interrupted && switched.is_none() {
            fresh = previous.session.is_none();
            session = previous.session;
        }
        previous.workspace
    } else if source.isolated {
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
    // A card that never reported a session cannot be continued, unless it is to start fresh.
    if session.is_none() && !fresh { return Err("이어갈 세션 ID가 없습니다".into()); }
    if session.as_deref().is_some_and(|session| !is_run_id(session)) { return Err("세션 ID가 올바르지 않습니다".into()); }
    Ok(Target { card, project: plan.project, workspace, session, clean: source.clean })
}

/// The existing folder a path names, links resolved, in the plain form the rest of O uses
/// (`store::resolved_path`: no extended-length prefix on Windows), so a resumed card's cwd,
/// plan project and Codex `-C` match the folders the first run used.
fn resolved(path: &Path, missing: &str) -> Result<PathBuf, String> {
    path.canonicalize().map_err(|_| missing.to_string())?;
    Ok(PathBuf::from(store::resolved_path(&path.to_string_lossy())))
}

/// Creates a policy-checked run that resumes the original card in its own workspace.
pub fn start(home: &Path, run: &str, card: &str, text: &str, spawn: &super::input::StartRun<'_>) -> Result<String, String> {
    if !is_run_id(run) { return Err("실행 ID가 올바르지 않습니다".into()); }
    let running = run_is_live(home, run) && !read_events(home, run).iter().any(|event| matches!(event, OEvent::CardEnd { card_id, .. } if card_id == card));
    let followup = Input::capture(home, run, card, running)?;
    let source = followup.sources[0].plan.clone();
    let mut next_card = source.cards.into_iter().find(|item| item.id == card).ok_or("해당 작업을 찾지 못했습니다")?;
    if crate::providers::get(next_card.provider).terminal_card() { return Err("터미널 카드는 후속 지시를 받지 않습니다".into()); }
    if !running { next_card = followup.target(home)?.card; }
    next_card.task = text.to_string();
    next_card.depends_on.clear();
    let plan = Plan { title: "후속 작업".into(), project: source.project, cards: vec![next_card] };
    let raw = serde_json::to_value(&plan).map_err(|e| e.to_string())?;
    let mut ctx = store::policy_context(home, Utc::now()).map_err(|e| e.to_string())?;
    // Queuing takes no execution slot; real active caps are checked again at launch.
    ctx.active_by_provider.clear();
    let profile = Profile::of_clean(followup.sources[0].clean);
    let checked = crate::policy::check_plan_for(&raw, &ctx, profile);
    let plan = checked.plan.ok_or_else(|| checked.violations.iter().map(|v| v.message.as_str()).collect::<Vec<_>>().join(" / "))?;
    let id = store::new_run_id(Utc::now());
    let mut input = super::input::RunInput::new(home, &id, &plan, RunMode { profile, context: true, yolo: false }, false, false);
    input.followup = Some(followup);
    store::create_run(home, &id, &plan).map_err(|e| e.to_string())?;
    store::mark_mode(home, &id, &RunMode { profile, context: true, yolo: false }).map_err(|e| e.to_string())?;
    // Whether it waits behind a running card is decided now, not when its runner starts:
    // a stop recorded first must still cancel it (`runner::wait_for_followup`).
    if running { fs::write(run_dir(home, &id).join(QUEUED_BEHIND), "").map_err(|e| e.to_string())?; }
    fs::write(run_dir(home, &id).join("followup.json"), serde_json::to_vec(&(run, card)).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let item = &plan.cards[0];
    let at = crate::sys::js::now_iso();
    for event in [
        OEvent::RunStart { at: at.clone(), run_id: id.clone(), title: plan.title.clone(), project: plan.project.clone(), cards: vec![CardInfo { id: item.id.clone(), provider: item.provider, label: Some(item.label()), role: item.role.clone(), model: item.model.clone(), effort: item.effort.clone() }] },
        OEvent::RunFollowup { at: at.clone(), source_run_id: run.into(), card_id: card.into() },
        OEvent::RunMode { at: at.clone(), run_id: id.clone(), profile, context: true, yolo: false, scheduled: None },
        OEvent::CardInput { at, card_id: card.into(), text: item.task.clone() },
    ] { store::append_event(home, &id, &event).map_err(|e| e.to_string())?; }
    if let Some(branch) = store::read_events(home, run).into_iter().rev().find_map(|event| match event {
        OEvent::RunBranch { branch, .. } => Some(branch),
        _ => None,
    }) {
        store::append_event(home, &id, &OEvent::RunBranch { at: crate::sys::js::now_iso(), branch }).map_err(|e| e.to_string())?;
    }
    if let Err(error) = spawn(&id, &input) {
        let card = &plan.cards[0];
        let at = crate::sys::js::now_iso();
        let _ = store::append_event(home, &id, &OEvent::CardEnd { at: at.clone(), card_id: card.id.clone(), status: CardStatus::Failed, result: Some(format!("실행기를 시작하지 못했습니다: {error}")), exit_code: None });
        let _ = store::append_event(home, &id, &OEvent::RunEnd { at, run_id: id.clone(), status: CardStatus::Failed });
        return Err(format!("후속 실행을 시작하지 못했습니다: {error}"));
    }
    Ok(id)
}
