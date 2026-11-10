//! File delivery and completion for interactive terminal cards.
use super::*;
use super::card::Slot;

#[cfg(windows)]
use sha2::{Digest, Sha256};

#[cfg(windows)]
const MAX_DONE: u64 = 1024 * 1024;
#[cfg(windows)]
const PASTE_LIMIT: Duration = Duration::from_secs(30 * 60);
#[cfg(windows)]
const WORK_LIMIT: Duration = Duration::from_secs(60 * 60);

#[cfg(windows)]
fn plain_dir(path: &Path) -> io::Result<()> {
    match fs::create_dir(path) { Ok(()) => {}, Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}, Err(e) => return Err(e) }
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.file_type().is_symlink() { return Err(io::Error::other("터미널 과제 폴더가 링크이거나 폴더가 아닙니다")); }
    Ok(())
}

#[cfg(windows)]
fn new_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    OpenOptions::new().write(true).create_new(true).open(path)?.write_all(bytes)
}

#[cfg(windows)]
struct Terminal<'a> { home: &'a Path, id: String, exited: Arc<AtomicBool> }
#[cfg(windows)]
impl Drop for Terminal<'_> {
    fn drop(&mut self) {
        let _ = crate::term::close(self.home, &self.id);
        let _ = crate::term::wait(self.home, &self.id, Duration::from_secs(10));
        self.exited.store(crate::term::tree_confirmed(self.home, &self.id), Ordering::Release);
    }
}

#[cfg(windows)]
fn read_done(path: &Path, folder: &Path) -> Option<String> {
    let parent = path.parent()?.canonicalize().ok()?;
    if parent != folder || path.canonicalize().ok()?.parent()? != parent { return None; }
    let file = crate::run::workspaces::open_plain(path).ok()?;
    let before = file.metadata().ok()?;
    if before.len() > MAX_DONE { return None; }
    let mut text = String::new();
    (&file).take(MAX_DONE + 1).read_to_string(&mut text).ok()?;
    let after = file.metadata().ok()?;
    if before.len() != after.len() || before.modified().ok()? != after.modified().ok()? { return None; }
    (text.len() as u64 <= MAX_DONE).then_some(text)
}

#[allow(clippy::too_many_arguments)]
#[cfg(windows)]
pub(super) fn run(home: &Path, run_id: &str, plan: &Plan, card: &Card, cwd: &str, context: Option<&str>, spawner: &Spawner, dependencies: &[(String, String)], mut slot: Slot<'_>) -> io::Result<CardStatus> {
    let emit = |event: OEvent| append_event(home, run_id, &event);
    let end = |status, result| OEvent::CardEnd { at: now_iso(), card_id: card.id.clone(), status, result, exit_code: None };
    if !crate::agents::agy::workers_allowed(home) {
        drop(slot);
        emit(end(CardStatus::Failed, Some(crate::policy::AGY_WORKERS_DISABLED.into())))?;
        return Ok(CardStatus::Failed);
    }
    if !crate::run::store::is_run_id(run_id) || !crate::run::store::is_run_id(&card.id) { return Err(io::Error::other("터미널 과제 ID가 올바르지 않습니다")); }
    let root = home.with_file_name(format!("{}-term", home.file_name().unwrap_or_default().to_string_lossy()));
    let run_folder = root.join(run_id);
    let folder = run_folder.join(&card.id);
    for path in [&root, &run_folder, &folder] { plain_dir(path)?; }
    if folder.canonicalize()?.starts_with(Path::new(cwd).canonicalize()?) { return Err(io::Error::other("터미널 과제 폴더가 작업 폴더 안에 있습니다")); }
    let task = folder.join("task.md");
    let done = folder.join("done.md");
    let checked_folder = folder.canonicalize()?;
    let nonce = (0..4).map(|_| crate::run::store::new_run_id(Utc::now())).collect::<Vec<_>>().join("-");
    let checked_plan = Plan { project: cwd.to_string(), ..plan.clone() };
    let run_dir = run_dir(home, run_id).to_string_lossy().into_owned();
    let team = TeamChannel { run_id, run_dir: &run_dir };
    let prompt = crate::providers::terminal_card_prompt(card, &checked_plan, &team, context);
    let text = format!("{prompt}\n\n## 완료 보고\n\n{} 폴더의 임시 파일에 보고서 전체를 쓰고 마지막에 {} 파일로 이름을 바꾸세요. 첫 줄과 마지막 비어 있지 않은 줄은 정확히 아래와 같아야 합니다. 두 줄 사이에 한 일, 바꾼 파일, 남은 문제를 적으세요.\n\nO-DONE {run_id}/{} {nonce}\n...\nO-END {nonce}\n", folder.display(), done.display(), card.id);
    let hash = format!("{:x}", Sha256::digest(text.as_bytes()));
    new_file(&task, text.as_bytes())?;
    let at = now_iso();
    let delivery = serde_json::json!({ "runId": run_id, "cardId": card.id, "nonce": nonce, "deliveredAt": at, "taskSha256": hash });
    new_file(&folder.join("delivery.json"), &serde_json::to_vec_pretty(&delivery).map_err(io::Error::other)?)?;
    let line = format!("{} 파일을 읽고 그대로 수행하세요. 끝나면 그 파일에 적힌 방법으로 완료를 알리세요.", task.display());
    let spec = LaunchSpec { bin: card.provider.as_str().into(), args: Vec::new(), stdin: None, env: Default::default() };
    let mut launch = spawner(&spec, cwd)?;
    launch.env("O_RUN_ID", run_id).env("O_CARD_ID", &card.id);
    crate::agents::billing::check(home, card.provider, &launch).map_err(io::Error::other)?;
    let watcher_executable = launch.handle_owner_executable.clone().map_or_else(std::env::current_exe, Ok)?;
    if !crate::agents::agy::workers_allowed(home) { return Err(io::Error::other(crate::policy::AGY_WORKERS_DISABLED)); }
    let mut terminal = None;
    let no_process = Arc::new(AtomicBool::new(true));
    let opened = crate::run::store::with_events(home, run_id, |file| {
        let events = crate::run::store::event_text(file)?;
        for (source, hash) in dependencies { crate::run::result::verify_event_result(&events, source, hash).map_err(io::Error::other)?; }
        crate::run::store::with_start_gate(home, run_id, || {
            if stop_requested(home, run_id) { return Err(io::Error::new(io::ErrorKind::Interrupted, "작업이 중지되었습니다")); }
            let settings = crate::agents::agy::report(home);
            if !settings.workers { return Err(io::Error::other(crate::policy::AGY_WORKERS_DISABLED)); }
            let confirm = settings.confirm_paste;
            crate::run::store::mark_spawning(home, &slot.reservation)?;
            slot.started = true;
            #[cfg(test)]
            let timing = if card.id == "timeout-test" { (Duration::from_millis(50), Duration::from_millis(500)) } else { (Duration::from_secs(10), Duration::ZERO) };
            let session = crate::term::open_card_tracked(home, card.provider.to_string(), cwd.into(), run_id.into(), card.id.clone(), launch, line, confirm, Arc::clone(&no_process), Arc::clone(&slot.exited), #[cfg(test)] timing)?;
            terminal = Some(Terminal { home, id: session.id.clone(), exited: Arc::clone(&slot.exited) });
            let (pid, job) = crate::term::card_worker(home, &session.id)?;
            link_slot(home, &slot.reservation, pid, job.as_deref())?;
            slot.marked = true;
            mark_active(home, &ActiveMarker { run_id: run_id.into(), card_id: card.id.clone(), provider: card.provider, pid })?;
            let start = OEvent::CardStart { at: now_iso(), card_id: card.id.clone(), pid: Some(pid) };
            crate::run::store::append_value(file, &serde_json::to_value(start).map_err(io::Error::other)?)?;
            Ok(confirm)
        })
    });
    let confirm = match opened {
        Ok(confirm) => confirm,
        Err(error) => {
            drop(terminal);
            if no_process.load(Ordering::Acquire) { slot.started = false; }
            if stop_requested(home, run_id) { drop(slot); emit(end(CardStatus::Stopped, None))?; return Ok(CardStatus::Stopped); }
            return Err(error);
        }
    };
    let terminal = terminal.expect("started terminal");
    let _ = crate::run::job_watch::spawn(home, slot.reservation.slot_row, &watcher_executable);
    emit(OEvent::CardTool { at: now_iso(), card_id: card.id.clone(), name: "터미널 과제 전달".into(), summary: format!("{at} · {} · {}", &hash[..12], task.display()) })?;
    if confirm { emit(OEvent::CardTool { at: now_iso(), card_id: card.id.clone(), name: "붙여넣기 승인 대기".into(), summary: terminal.id.clone() })?; }
    let waiting_since = Instant::now();
    let mut pasted_at = None;
    let mut warned = false;
    let mut observed_exit = false;
    let mut stable = crate::term::Stability::default();
    let (status, result) = loop {
        if stop_requested(home, run_id) { break (CardStatus::Stopped, None); }
        let waiting_file = fs::metadata(&done).ok().is_some_and(|metadata| !stable.ready(&metadata, Duration::from_millis(500)));
        if !waiting_file { if let Some(text) = read_done(&done, &checked_folder) {
            let (first, body) = text.split_once('\n').unwrap_or((&text, ""));
            if first.trim_end_matches('\r') == format!("O-DONE {run_id}/{} {nonce}", card.id) {
                let lines: Vec<_> = body.lines().collect();
                if let Some(last) = lines.iter().rposition(|line| !line.trim().is_empty()) {
                    if lines[last] == format!("O-END {nonce}") {
                        let content = lines[..last].join("\n");
                        break (CardStatus::Done, Some(format!("{}\n완료 문서: {}", content.trim_end(), done.display())));
                    }
                }
            } else if !warned { emit(OEvent::CardTool { at: now_iso(), card_id: card.id.clone(), name: "확인되지 않은 완료 문서".into(), summary: done.to_string_lossy().into_owned() })?; warned = true; }
        } }
        if pasted_at.is_none() && crate::term::pasted(home, &terminal.id) { pasted_at = Some(Instant::now()); }
        let session = crate::term::get(home, &terminal.id)?;
        if session.state == "exited" {
            if observed_exit && !waiting_file { break (CardStatus::Failed, Some("완료 보고 없이 끝났습니다".into())); }
            observed_exit = true;
        }
        if pasted_at.is_some_and(|at| at.elapsed() >= WORK_LIMIT) { break (CardStatus::Failed, Some("터미널 카드 작업 제한 시간이 지났습니다".into())); }
        if session.state == "awaiting-paste" && waiting_since.elapsed() >= PASTE_LIMIT { break (CardStatus::Failed, Some("붙여넣기 승인 제한 시간이 지났습니다".into())); }
        thread::sleep(Duration::from_millis(100));
    };
    drop(terminal);
    let (status, result) = if status == CardStatus::Done && !slot.exited.load(Ordering::Acquire) {
        (CardStatus::Failed, Some("터미널 프로세스 트리의 종료를 확인하지 못했습니다".into()))
    } else { (status, result) };
    drop(slot);
    emit(end(status, result))?;
    Ok(status)
}

#[allow(clippy::too_many_arguments)]
#[cfg(not(windows))]
pub(super) fn run(_: &Path, _: &str, _: &Plan, _: &Card, _: &str, _: Option<&str>, _: &Spawner, _: &[(String, String)], _: Slot<'_>) -> io::Result<CardStatus> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "이 운영체제에서는 터미널을 지원하지 않습니다"))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn completion_file_must_be_plain_and_bounded() {
        let folder = std::env::temp_dir().join(format!("o-terminal-done-{}", crate::run::store::new_run_id(Utc::now())));
        fs::create_dir(&folder).unwrap();
        let checked = folder.canonicalize().unwrap();
        let done = folder.join("done.md");
        fs::write(&done, "complete").unwrap();
        assert_eq!(read_done(&done, &checked).as_deref(), Some("complete"));
        fs::hard_link(&done, folder.join("linked.md")).unwrap();
        assert!(read_done(&done, &checked).is_none());
        fs::remove_file(folder.join("linked.md")).unwrap();
        fs::OpenOptions::new().write(true).open(&done).unwrap().set_len(MAX_DONE + 1).unwrap();
        assert!(read_done(&done, &checked).is_none());
    }

    #[test]
    fn started_then_timed_out_releases_slot_without_refunding_start() {
        let base = std::env::temp_dir().join(format!("o-terminal-timeout-{}", crate::run::store::new_run_id(Utc::now())));
        let home = base.join("state");
        let project = base.join("project");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&project).unwrap();
        crate::agents::agy::set(&home, crate::agents::agy::Change { workers: Some(true), ack: Some(true), ..Default::default() }).unwrap();
        let card: Card = serde_json::from_value(serde_json::json!({"id":"timeout-test","provider":"agy","task":"test"})).unwrap();
        let project = project.to_string_lossy().into_owned();
        let plan = Plan { title: "test".into(), project: project.clone(), cards: vec![card.clone()] };
        crate::run::store::create_run(&home, "timeout-run", &plan).unwrap();
        let Reserved::Granted(reservation) = reserve_slot(&home, "timeout-run", &card, &project, Utc::now()).unwrap() else { panic!("reservation refused") };
        let slot = Slot { home: &home, reservation, started: false, marked: false, exited: Arc::new(AtomicBool::new(false)), best_effort: false };
        let spawner = |_: &LaunchSpec, cwd: &str| {
            let executable = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32").join("cmd.exe");
            let mut launch = Launch::new(executable);
            launch.arg("/K").current_dir(cwd);
            Ok(launch)
        };
        let error = run(&home, "timeout-run", &plan, &card, &project, None, &spawner, &[], slot).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(!crate::run::store::resource_held(&home, "timeout-run", &card.id).unwrap());
        assert_eq!(crate::run::store::started_on(&home, Utc::now()).unwrap(), 1);
    }
}
