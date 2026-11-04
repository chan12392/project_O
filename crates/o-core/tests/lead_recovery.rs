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
}

#[test]
fn a_finished_turn_allows_the_next_turn() {
    let (home, project) = fixture();
    let spawner = |_: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<Command> {
        let mut cmd = Command::new(std::env::current_exe()?);
        cmd.args(["--exact", "fake_child"]).env("O_LEAD_RECOVERY_SHORT", "1");
        Ok(cmd)
    };
    lead::send(&home, &request(&project), &spawner).unwrap();
    assert!(lead::show(&home, Some(&project))["thread"]["busy"].is_null());
    lead::send(&home, &request(&project), &spawner).unwrap();
    assert!(lead::show(&home, Some(&project))["thread"]["busy"].is_null());
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
