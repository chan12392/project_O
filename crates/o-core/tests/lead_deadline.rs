mod support;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use o_core::lead::{self, LeadEvent, SendRequest};
use o_core::run::plan::ProviderId;

#[test]
fn unresolved_turn_blocks_new_thread_only_for_its_project() {
    let root = std::env::temp_dir().join(format!("o-lead-unresolved-{}", std::process::id()));
    let home = root.join("home");
    let project = root.join("project").to_string_lossy().into_owned();
    let other = root.join("other").to_string_lossy().into_owned();
    let thread = lead::thread_for(&home, &project, false).unwrap();
    std::fs::write(lead::thread_dir(&home, &thread).join("running.json"),
        r#"{"provider":"claude","owner":1,"token":"test","unresolved":true}"#).unwrap();
    assert!(lead::thread_for(&home, &project, true).is_err());
    assert!(lead::thread_for(&home, &other, true).is_ok());
    assert_eq!(lead::current_thread(&home, &project).as_deref(), Some(thread.as_str()));
}

#[test]
fn older_unresolved_thread_blocks_start_in_current_thread() {
    let root = std::env::temp_dir().join(format!("o-lead-older-unresolved-{}", std::process::id()));
    let home = root.join("home");
    let project_dir = root.join("project");
    std::fs::create_dir_all(&project_dir).unwrap();
    let project = project_dir.to_string_lossy().into_owned();
    let old = lead::thread_for(&home, &project, false).unwrap();
    let current = lead::thread_for(&home, &project, true).unwrap();
    std::fs::write(lead::thread_dir(&home, &old).join("running.json"),
        r#"{"provider":"claude","owner":1,"token":"test","unresolved":true}"#).unwrap();
    let request = SendRequest { project: &project, provider: ProviderId::Claude, model: None, effort: None, text: "test", attach: &[], clean: None, fresh: false, yolo: false };
    let spawn = |_: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<o_core::sys::launch::Launch> { panic!("unresolved turn must block the provider") };
    assert!(lead::send_within(&home, &request, &spawn, Duration::from_secs(1)).unwrap_err().contains("확인할 수 없어"));
    assert_eq!(lead::current_thread(&home, &project).as_deref(), Some(current.as_str()));
}

#[test]
fn live_turn_blocks_start_in_new_thread_until_released() {
    let root = std::env::temp_dir().join(format!("o-lead-busy-thread-{}", std::process::id()));
    let home = root.join("home");
    let project_dir = root.join("project");
    std::fs::create_dir_all(&project_dir).unwrap();
    let project = project_dir.to_string_lossy().into_owned();
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let lock = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false)
        .open(lead::thread_dir(&home, &thread).join("turn.lock")).unwrap();
    lock.lock().unwrap();
    assert!(lead::thread_for(&home, &project, true).is_err());
    let request = SendRequest { project: &project, provider: ProviderId::Claude, model: None, effort: None, text: "test", attach: &[], clean: None, fresh: false, yolo: false };
    let spawn = |_: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<o_core::sys::launch::Launch> { panic!("live turn must block the provider") };
    assert!(lead::send_within(&home, &request, &spawn, Duration::from_secs(1)).unwrap_err().contains("답하는 중"));
    drop(lock);
    let next = lead::thread_for(&home, &project, true).unwrap();
    assert_eq!(lead::current_thread(&home, &project).as_deref(), Some(next.as_str()));
}

#[test]
#[allow(clippy::zombie_processes)] // The fixture deliberately orphans its pipe-holding child.
fn closed_stdout_fixture() {
    if std::env::var_os("O_TEST_CLOSED_STDOUT").is_none() { return; }
    if std::env::var_os("O_TEST_HOLD_PIPE").is_some() {
        std::thread::sleep(Duration::from_secs(5));
        return;
    }
    if std::env::var_os("O_TEST_DESCENDANT").is_some() {
        std::thread::sleep(Duration::from_millis(200));
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "closed_stdout_fixture", "--nocapture"])
            .env("O_TEST_HOLD_PIPE", "1").stdin(Stdio::null()).stderr(Stdio::inherit());
        if std::env::var_os("O_TEST_HOLD_STDOUT").is_none() { command.stdout(Stdio::null()); }
        let descendant = o_core::sys::proc::hide_window(&mut command).spawn().unwrap();
        if let Some(path) = std::env::var_os("O_TEST_DESC_PID") { std::fs::write(path, descendant.id().to_string()).unwrap(); }
        return;
    }
    if std::env::var_os("O_TEST_PARTIAL").is_some() {
        println!("{{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"partial\"}}");
        println!("{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\"partial output\"}}]}}}}");
    }
    #[cfg(windows)]
    unsafe {
        use std::os::windows::io::AsRawHandle;
        windows_sys::Win32::Foundation::CloseHandle(std::io::stdout().as_raw_handle() as _);
    }
    #[cfg(unix)]
    unsafe {
        unsafe extern "C" { fn close(fd: i32) -> i32; }
        close(1);
    }
    if let Some(path) = std::env::var_os("O_TEST_READY") { std::fs::write(path, "ready").unwrap(); }
    std::thread::sleep(Duration::from_secs(8));
}

#[test]
fn closed_stdout_turn() {
    if std::env::var_os("O_TEST_DEADLINE_TURN").is_none() { return; }
    let root = std::env::temp_dir().join(format!("o-lead-deadline-{}", std::process::id()));
    let home = root.join("home");
    let project = root.join("project");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let project = project.to_string_lossy();
    let request = SendRequest { project: &project, provider: ProviderId::Claude, model: None, effort: None, text: "test", attach: &[], clean: None, fresh: false, yolo: false };
    let ready = root.join("ready");
    let descendant_pid = root.join("descendant.pid");
    let mode = std::env::var("O_TEST_MODE").unwrap_or_default();
    let (spawn_mode, spawn_ready, spawn_pid) = (mode.clone(), ready.clone(), descendant_pid.clone());
    let spawn = move |_: &o_core::providers::LaunchSpec, _: &str| {
        let mut command = support::fake_worker_launch()?;
        command.args(["--exact", "closed_stdout_fixture", "--nocapture"]).env("O_TEST_CLOSED_STDOUT", "1");
        if spawn_mode == "descendant" || spawn_mode == "both" { command.env("O_TEST_DESCENDANT", "1").env("O_TEST_DESC_PID", &spawn_pid); }
        if spawn_mode == "both" { command.env("O_TEST_HOLD_STDOUT", "1"); }
        if spawn_mode == "stop" { command.env("O_TEST_READY", &spawn_ready); }
        if spawn_mode == "partial" { command.env("O_TEST_PARTIAL", "1"); }
        Ok(command)
    };
    let stopper = if mode == "stop" {
        let (home, project) = (home.clone(), project.to_string());
        Some(std::thread::spawn(move || {
            while !ready.exists() { std::thread::sleep(Duration::from_millis(20)); }
            assert!(lead::stop(&home, Some(&project)));
        }))
    } else { None };
    let limit = if mode == "stop" { Duration::from_secs(10) } else { Duration::from_millis(800) };
    let events = lead::send_within(&home, &request, &spawn, limit).unwrap();
    if let Some(stopper) = stopper { stopper.join().unwrap(); }
    let expected = if mode == "stop" { "중지" } else { "시간" };
    assert!(events.iter().any(|event| matches!(event, LeadEvent::Error { text, .. } if text.contains(expected))), "{events:?}");
    #[cfg(windows)]
    if mode == "descendant" || mode == "both" {
        assert!(events.iter().any(|event| matches!(event, LeadEvent::Error { text, .. } if text.contains("멈췄습니다"))), "{events:?}");
        let pid = std::fs::read_to_string(descendant_pid).unwrap().parse().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while o_core::sys::proc::is_alive(pid) {
            assert!(Instant::now() < deadline, "job left its descendant running");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    if mode == "partial" {
        let thread = lead::current_thread(&home, &project).unwrap();
        let raw = std::fs::read_to_string(lead::thread_dir(&home, &thread).join("turn.raw.jsonl")).unwrap();
        assert!(raw.contains("partial output"), "{raw}");
        assert!(!events.iter().any(|event| matches!(event, LeadEvent::Reply { .. })));
    }
}

#[test]
fn deadline_survives_stdout_eof() {
    run_turn("");
}

#[test]
fn deadline_covers_inherited_stderr() { run_turn("descendant"); }

#[test]
fn deadline_covers_inherited_stdout_and_stderr() { run_turn("both"); }

#[test]
fn stop_survives_stdout_eof() { run_turn("stop"); }

#[test]
fn timeout_preserves_partial_raw_output() { run_turn("partial"); }

fn run_turn(mode: &str) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", "closed_stdout_turn", "--nocapture"]).env("O_TEST_DEADLINE_TURN", "1").env("O_TEST_MODE", mode).stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = o_core::sys::proc::Contained::spawn(&mut command).unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if let Some(status) = child.child.try_wait().unwrap() {
            assert!(status.success(), "turn fixture failed: {status}");
            return;
        }
        assert!(Instant::now() < deadline, "turn remained blocked after stdout EOF");
        std::thread::sleep(Duration::from_millis(25));
    }
}
