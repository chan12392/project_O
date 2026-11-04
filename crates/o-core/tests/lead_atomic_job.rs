#![cfg(windows)]
mod support;

use o_core::lead::{self, LeadEvent, SendRequest};
use o_core::run::plan::ProviderId;
use std::path::PathBuf;
use std::time::Duration;

fn fixture() -> (PathBuf, String) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!("o-lead-atomic-{}-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    (root.join("home"), project.to_string_lossy().into_owned())
}
fn request(project: &str) -> SendRequest<'_> {
    SendRequest { project, provider: ProviderId::Claude, model: None, effort: None, text: "test", attach: &[], fresh: false, clean: None, yolo: false }
}

#[test]
fn fake_child() {
    if let Some(path) = std::env::var_os("O_LEAD_ATOMIC_STARTED") { std::fs::write(path, "started").unwrap(); }
    if std::env::var_os("O_LEAD_ATOMIC_WAIT").is_some() { std::thread::sleep(Duration::from_secs(30)); }
    if std::env::var_os("O_LEAD_ATOMIC_STARTED").is_some() { println!("{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"ok\"}}"); }
}

#[test]
fn intent_marker_failure_does_not_spawn_or_block_next_turn() {
    let (home, project) = fixture();
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let running = lead::thread_dir(&home, &thread).join("running.json");
    let marker = PathBuf::from(&project).join("child-started");
    let output = marker.clone();
    let dir = project.clone();
    let fail = move |_: &o_core::providers::LaunchSpec, _: &str| {
        let data: serde_json::Value = serde_json::from_slice(&std::fs::read(&running)?).unwrap();
        std::fs::create_dir(running.with_extension(format!("{}.tmp", data["token"].as_str().unwrap())))?;
        let mut launch = support::fake_worker_launch()?;
        launch.current_dir(&dir).args(["--exact", "fake_child", "--nocapture"]).env("O_LEAD_ATOMIC_STARTED", &output);
        Ok(launch)
    };
    let events = lead::send(&home, &request(&project), &fail).unwrap();
    assert!(events.iter().any(|e| matches!(e, LeadEvent::Error { .. })));
    assert!(!marker.exists());
    assert!(lead::show(&home, Some(&project))["thread"]["busy"].is_null());
    assert!(lead::thread_for(&home, &project, true).is_ok());
}

#[test]
fn invalid_helper_never_starts_a_lead() {
    let (home, project) = fixture();
    let marker = PathBuf::from(&project).join("child-started");
    let output = marker.clone();
    let dir = project.clone();
    let bad = move |_: &o_core::providers::LaunchSpec, _: &str| {
        let mut launch = support::fake_worker_launch()?;
        launch.handle_owner_executable = Some(output.with_file_name("missing-helper.exe"));
        launch.current_dir(&dir).args(["--exact", "fake_child", "--nocapture"]).env("O_LEAD_ATOMIC_STARTED", &output);
        Ok(launch)
    };
    let events = lead::send(&home, &request(&project), &bad).unwrap();
    assert!(events.iter().any(|e| matches!(e, LeadEvent::Error { .. })));
    assert!(!marker.exists());
    assert!(lead::show(&home, Some(&project))["thread"]["busy"].is_null());
    assert!(lead::thread_for(&home, &project, true).is_ok());
}

#[test]
fn stop_ends_contained_lead_and_clears_marker() {
    let (home, project) = fixture();
    let marker = PathBuf::from(&project).join("child-started");
    let (worker_home, worker_project, output) = (home.clone(), project.clone(), marker.clone());
    let worker = std::thread::spawn(move || {
        let dir = worker_project.clone();
        let spawn = move |_: &o_core::providers::LaunchSpec, _: &str| {
            let mut launch = support::fake_worker_launch()?;
            launch.current_dir(&dir).args(["--exact", "fake_child", "--nocapture"]).env("O_LEAD_ATOMIC_STARTED", &output).env("O_LEAD_ATOMIC_WAIT", "1");
            Ok(launch)
        };
        lead::send(&worker_home, &request(&worker_project), &spawn).unwrap()
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !marker.exists() { assert!(std::time::Instant::now() < deadline); std::thread::sleep(Duration::from_millis(10)); }
    assert!(lead::thread_for(&home, &project, true).is_err());
    assert!(lead::stop(&home, Some(&project)));
    let events = worker.join().unwrap();
    assert!(events.iter().any(|e| matches!(e, LeadEvent::Error { text, .. } if text.contains("중지"))));
    assert!(lead::show(&home, Some(&project))["thread"]["busy"].is_null());
    assert!(lead::thread_for(&home, &project, true).is_ok());
}
