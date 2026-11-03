use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use o_core::lead::{self, SendRequest};
use o_core::run::plan::ProviderId;

fn fixture() -> (PathBuf, String) {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("o-lead-recovery-{}-{unique}", std::process::id()));
    let project = base.join("project");
    std::fs::create_dir_all(&project).unwrap();
    (base.join("state"), project.to_string_lossy().into_owned())
}

fn request(project: &str) -> SendRequest<'_> {
    SendRequest { project, provider: ProviderId::Claude, model: None, effort: None, text: "test", attach: &[], fresh: false, clean: None, yolo: false }
}

#[test]
fn fake_child() {
    if std::env::var_os("O_LEAD_RECOVERY_WAIT").is_some() { std::thread::sleep(Duration::from_secs(30)); }
    if std::env::var_os("O_LEAD_RECOVERY_SHORT").is_some() { std::thread::sleep(Duration::from_millis(500)); }
    if let Some(path) = std::env::var_os("O_LEAD_STATUS_PATH") {
        use std::io::Write;
        let source = std::fs::read_to_string(&path).unwrap();
        assert!(source.contains("LAST-LINE-한글"));
        assert!(std::fs::write(path, b"tampered").is_err());
        let seen = std::env::var_os("O_LEAD_STATUS_SEEN").unwrap();
        writeln!(std::fs::OpenOptions::new().create(true).append(true).open(seen).unwrap(), "read").unwrap();
    }
}

#[test]
fn orphan_and_legacy_markers_never_authorize_a_kill_or_new_turn() {
    let (home, project) = fixture();
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let marker = lead::thread_dir(&home, &thread).join("running.json");
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", "fake_child"]).env("O_LEAD_RECOVERY_WAIT", "1").stdout(Stdio::null()).stderr(Stdio::null());
    let mut other = o_core::sys::proc::hide_window(&mut command).spawn().unwrap();
    let pid = other.id();
    let started = o_core::sys::proc::started_at(pid).unwrap();
    for content in [
        serde_json::json!({ "provider": "claude", "owner": 123, "token": "old", "child": { "pid": pid, "started": started } }).to_string(),
        serde_json::json!({ "provider": "claude", "owner": 123, "token": "old", "child": { "pid": pid, "started": started + 1 } }).to_string(),
        serde_json::json!({ "provider": "claude", "owner": 123, "token": "old", "child": { "pid": pid } }).to_string(),
        "{broken".to_string(),
    ] {
        std::fs::write(&marker, &content).unwrap();
        assert_eq!(lead::show(&home, Some(&project))["thread"]["busy"], "unknown");
        assert!(!lead::stop(&home, Some(&project)));
        assert!(lead::new_thread(&home, &project).is_err());
        let spawner = |_: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<Command> { panic!("unexpected spawn") };
        assert!(lead::send(&home, &request(&project), &spawner).unwrap_err().contains("종료를 확인"));
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), content);
        assert!(other.try_wait().unwrap().is_none());
    }
    other.kill().unwrap();
    other.wait().unwrap();
}

#[cfg(windows)]
#[test]
fn simultaneous_fresh_turns_start_at_most_one_child() {
    use std::sync::{Arc, Barrier, atomic::{AtomicUsize, Ordering}};
    let (home, project) = fixture();
    let barrier = Arc::new(Barrier::new(3));
    let starts = Arc::new(AtomicUsize::new(0));
    let workers: Vec<_> = (0..2).map(|_| {
        let (home, project, barrier, starts) = (home.clone(), project.clone(), Arc::clone(&barrier), Arc::clone(&starts));
        std::thread::spawn(move || {
            let spawner = move |_: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<Command> {
                starts.fetch_add(1, Ordering::SeqCst);
                let mut cmd = Command::new(std::env::current_exe()?);
                cmd.args(["--exact", "fake_child"]).env("O_LEAD_RECOVERY_SHORT", "1");
                Ok(cmd)
            };
            barrier.wait();
            lead::send(&home, &SendRequest { fresh: true, ..request(&project) }, &spawner)
        })
    }).collect();
    barrier.wait();
    for worker in workers { let _ = worker.join().unwrap(); }
    assert_eq!(starts.load(Ordering::SeqCst), 1);
}

#[cfg(windows)]
#[test]
fn live_owner_stops_its_contained_child_and_releases_the_turn() {
    let (home, project) = fixture();
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let marker = lead::thread_dir(&home, &thread).join("running.json");
    let (worker_home, worker_project) = (home.clone(), project.clone());
    let worker = std::thread::spawn(move || {
        let spawner = |_: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<Command> {
            let mut cmd = Command::new(std::env::current_exe()?);
            cmd.args(["--exact", "fake_child"]).env("O_LEAD_RECOVERY_WAIT", "1");
            Ok(cmd)
        };
        lead::send(&worker_home, &request(&worker_project), &spawner).unwrap()
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !std::fs::read_to_string(&marker).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()).is_some_and(|v| v.get("child").is_some()) {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(lead::new_thread(&home, &project).is_err());
    assert!(lead::stop(&home, Some(&project)));
    worker.join().unwrap();
    assert!(!marker.exists());
    assert!(lead::show(&home, Some(&project))["thread"]["busy"].is_null());
}

#[cfg(windows)]
#[test]
fn lead_reads_complete_results_and_reuses_an_unchanged_snapshot() {
    use std::io::Write;
    use std::sync::{Arc, Mutex};
    use o_core::run::events::{CardInfo, CardStatus, OEvent};
    use o_core::run::plan::{Card, Plan};
    use o_core::run::store::{append_event, create_run};
    let (home, project) = fixture();
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let end = "LAST-LINE-한글";
    let result = format!("{}\n{end}", "long data\n".repeat(450));
    let plan = Plan { title: "test".into(), project: project.clone(), cards: vec![Card { id: "card".into(), provider: ProviderId::Claude, role: None, task: "test".into(), model: None, effort: None, depends_on: vec![], resources: vec![] }] };
    let log = lead::thread_dir(&home, &thread).join("events.jsonl");
    for index in 0..12 {
        let id = format!("run-{index}");
        create_run(&home, &id, &plan).unwrap();
        append_event(&home, &id, &OEvent::RunStart { at: "now".into(), run_id: id.clone(), title: "test".into(), project: project.clone(), cards: vec![CardInfo { id: "card".into(), provider: ProviderId::Claude, label: None, role: None, model: None, effort: None }] }).unwrap();
        append_event(&home, &id, &OEvent::CardEnd { at: "now".into(), card_id: "card".into(), status: CardStatus::Failed, result: Some(result.clone()), exit_code: Some(1) }).unwrap();
        append_event(&home, &id, &OEvent::RunEnd { at: "now".into(), run_id: id.clone(), status: CardStatus::Failed }).unwrap();
        let dispatch = serde_json::json!({"t":"lead.dispatch", "at":"now", "planId":"p", "runId":id, "clean":false});
        writeln!(std::fs::OpenOptions::new().append(true).open(&log).unwrap(), "{dispatch}").unwrap();
    }
    let observations = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&observations);
    let evidence = home.parent().unwrap().join("child-read.txt");
    let child_evidence = evidence.clone();
    let spawner = move |spec: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<Command> {
        let prompt = spec.stdin.as_ref().unwrap();
        assert!(prompt.contains("생략"));
        assert!(prompt.contains("신뢰할 수 없는"));
        assert!(!prompt.contains(end));
        let tools = spec.args.iter().position(|a| a == "--tools").unwrap();
        assert_eq!(spec.args[tools + 1], "Read,Grep,Glob");
        let index = spec.args.iter().position(|a| a == "--add-dir").unwrap();
        let dir = PathBuf::from(&spec.args[index + 1]);
        let files: Vec<_> = std::fs::read_dir(&dir)?.map(|e| e.unwrap().path()).filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("run-status-")).collect();
        assert_eq!(files.len(), 1);
        let source: serde_json::Value = serde_json::from_slice(&std::fs::read(&files[0])?)?;
        let runs = source["runs"].as_array().unwrap();
        assert_eq!(runs.len(), 12);
        for run in runs { assert_eq!(run["resultEvents"][1]["result"], result); }
        assert!(std::fs::write(&files[0], b"tampered").is_err());
        captured.lock().unwrap().push(files[0].clone());
        let mut command = Command::new(std::env::current_exe()?);
        command.args(["--exact", "fake_child"]).env("O_LEAD_STATUS_PATH", &files[0]).env("O_LEAD_STATUS_SEEN", &child_evidence);
        Ok(command)
    };
    lead::send(&home, &request(&project), &spawner).unwrap();
    lead::send(&home, &request(&project), &spawner).unwrap();
    let observed = observations.lock().unwrap();
    assert_eq!(observed.len(), 2);
    assert_eq!(observed[0], observed[1]);
    assert_eq!(std::fs::read_to_string(evidence).unwrap().lines().count(), 2);
}

#[cfg(windows)]
#[test]
fn unreadable_result_record_is_reported_before_starting_the_lead() {
    use std::io::Write;
    let (home, project) = fixture();
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let run_dir = o_core::run::store::run_dir(&home, "broken");
    std::fs::create_dir_all(&run_dir).unwrap();
    std::fs::write(run_dir.join("events.jsonl"), "{not-json}\n").unwrap();
    let log = lead::thread_dir(&home, &thread).join("events.jsonl");
    writeln!(std::fs::OpenOptions::new().append(true).open(log).unwrap(), "{}", serde_json::json!({"t":"lead.dispatch", "at":"now", "planId":"p", "runId":"broken", "clean":false})).unwrap();
    let spawner = |_: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<Command> { panic!("should not start") };
    let error = lead::send(&home, &request(&project), &spawner).unwrap_err();
    assert!(error.contains("기록을 읽지 못했습니다"), "{error}");
}
