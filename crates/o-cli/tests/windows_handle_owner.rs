#[cfg(windows)]
#[test]
fn cli_handle_owner_signals_readiness_and_wrong_entry_fails() {
    let executable = std::path::PathBuf::from(env!("CARGO_BIN_EXE_o"));
    assert!(o_core::sys::win_launch::HandleOwner::start(executable.clone(), "--o-handle-owner").is_ok());
    assert!(o_core::sys::win_launch::HandleOwner::start(executable, "--bad-handle-owner-entry").is_err());
}

#[cfg(windows)]
#[test]
fn card_fixture() {
    if let Some(marker) = std::env::var_os("O_FAKE_OWNER_MARKER") {
        std::fs::write(marker, "ran").unwrap();
        println!(r#"{{"type":"result","subtype":"success","is_error":false,"result":"ok"}}"#);
    }
}

#[cfg(windows)]
#[test]
fn card_uses_cli_handle_owner_and_rejects_wrong_helper() {
    use std::sync::Arc;
    use o_core::run::events::CardStatus;
    use o_core::run::plan::{Card, Plan, ProviderId, Resource, ResourceKind, ResourceMode};
    use o_core::run::runner::{run_plan, Spawner};
    use o_core::run::store::{create_run, resource_held};
    for valid in [true, false] {
        let base = std::env::temp_dir().join(format!("o-owner-card-{}-{}-{}", std::process::id(), valid, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let project = base.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let home = base.join("state");
        let marker = project.join("ran");
        let card = Card { id: "card".into(), provider: ProviderId::Claude, role: None, task: "test".into(), model: None, effort: None, depends_on: vec![], resources: vec![Resource { kind: ResourceKind::External, key: "test".into(), mode: ResourceMode::Write }] };
        let plan = Plan { title: "test".into(), project: project.to_string_lossy().into_owned(), cards: vec![card] };
        create_run(&home, "run", &plan).unwrap();
        let fake_marker = marker.clone();
        let spawner: Arc<Spawner> = Arc::new(move |_, _| {
            let mut launch = o_core::sys::launch::Launch::new(std::env::current_exe()?);
            launch.args(["--exact", "card_fixture", "--nocapture"]).env("O_FAKE_OWNER_MARKER", &fake_marker);
            launch.handle_owner_executable = Some(if valid { env!("CARGO_BIN_EXE_o").into() } else { fake_marker.with_file_name("missing-helper.exe") });
            Ok(launch)
        });
        let status = run_plan(&home, "run", &plan, None, spawner).unwrap();
        assert_eq!(status, if valid { CardStatus::Done } else { CardStatus::Failed });
        assert_eq!(marker.exists(), valid);
        assert!(!resource_held(&home, "run", "card").unwrap());
    }
}

#[cfg(windows)]
#[test]
fn actual_cli_runner_uses_its_own_helper_for_fake_provider() {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    use o_core::run::events::{CardStatus, OEvent};
    use o_core::run::plan::{Card, Plan, ProviderId, Resource, ResourceKind, ResourceMode};
    use o_core::run::store::{create_run, read_events, resource_held};
    let base = std::env::temp_dir().join(format!("o-real-runner-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let project = base.join("project");
    let bin = base.join("bin");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    let source = base.join("fake.rs");
    std::fs::write(&source, r##"fn main() {
        if std::env::var_os("ANTHROPIC_API_KEY").is_some() { std::process::exit(9); }
        std::fs::write(std::env::var_os("O_FAKE_MARKER").unwrap(), "ok").unwrap();
        println!(r#"{{"type":"result","subtype":"success","is_error":false,"result":"ok"}}"#);
    }"##).unwrap();
    let status = Command::new("rustc").arg(&source).arg("-o").arg(bin.join("claude.exe"))
        .creation_flags(0x08000000).status().unwrap();
    assert!(status.success());
    let home = base.join("local").join("project_O");
    let card = Card { id: "card".into(), provider: ProviderId::Claude, role: None, task: "test".into(), model: None, effort: None, depends_on: vec![], resources: vec![Resource { kind: ResourceKind::External, key: "test".into(), mode: ResourceMode::Write }] };
    let plan = Plan { title: "test".into(), project: project.to_string_lossy().into_owned(), cards: vec![card] };
    create_run(&home, "run", &plan).unwrap();
    let marker = project.join("ran");
    let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()))).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_o")).args(["_run", "run"])
        .env("LOCALAPPDATA", base.join("local")).env("PATH", path).env("O_FAKE_MARKER", &marker)
        .env("ANTHROPIC_API_KEY", "fake-only").creation_flags(0x08000000).output().unwrap();
    assert!(output.status.success(), "runner failed: {}", String::from_utf8_lossy(&output.stderr));
    assert!(marker.exists(), "fake CLI was not started");
    assert!(read_events(&home, "run").iter().any(|event| matches!(event, OEvent::CardEnd { status: CardStatus::Done, .. })));
    assert!(!resource_held(&home, "run", "card").unwrap());
}

#[cfg(windows)]
#[test]
fn killed_actual_runner_stops_worker_and_preserves_reservation() {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    use o_core::run::plan::{Card, Plan, ProviderId, Resource, ResourceKind, ResourceMode};
    use o_core::run::store::{create_run, resource_held};
    struct Runner(std::process::Child);
    impl Drop for Runner { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }
    let base = std::env::temp_dir().join(format!("o-killed-runner-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let project = base.join("project");
    let bin = base.join("bin");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    let source = base.join("fake.rs");
    std::fs::write(&source, r#"fn main() {
        std::fs::write(std::env::var_os("O_FAKE_MARKER").unwrap(), std::process::id().to_string()).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(30));
    }"#).unwrap();
    assert!(Command::new("rustc").arg(&source).arg("-o").arg(bin.join("claude.exe")).creation_flags(0x08000000).status().unwrap().success());
    let home = base.join("local").join("project_O");
    let card = Card { id: "card".into(), provider: ProviderId::Claude, role: None, task: "test".into(), model: None, effort: None, depends_on: vec![], resources: vec![Resource { kind: ResourceKind::External, key: "test".into(), mode: ResourceMode::Write }] };
    let plan = Plan { title: "test".into(), project: project.to_string_lossy().into_owned(), cards: vec![card] };
    create_run(&home, "run", &plan).unwrap();
    let marker = project.join("pid");
    let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()))).unwrap();
    let mut runner = Runner(Command::new(env!("CARGO_BIN_EXE_o")).args(["_run", "run"]).env("LOCALAPPDATA", base.join("local"))
        .env("PATH", path).env("O_FAKE_MARKER", &marker).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .creation_flags(0x08000000).spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(15);
    while !marker.exists() { assert!(Instant::now() < deadline, "fake worker did not start"); std::thread::sleep(Duration::from_millis(20)); }
    let pid: u32 = std::fs::read_to_string(&marker).unwrap().parse().unwrap();
    let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    assert!(!raw.is_null());
    let worker = unsafe { OwnedHandle::from_raw_handle(raw) };
    runner.0.kill().unwrap();
    runner.0.wait().unwrap();
    assert_eq!(unsafe { WaitForSingleObject(worker.as_raw_handle(), 10_000) }, WAIT_OBJECT_0);
    assert!(resource_held(&home, "run", "card").unwrap());
}

#[cfg(windows)]
#[test]
fn actual_card_respects_nested_job_process_limit() {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use windows_sys::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE};
    use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};
    use o_core::run::events::{CardStatus, OEvent};
    use o_core::run::plan::{Card, Plan, ProviderId, Resource, ResourceKind, ResourceMode};
    use o_core::run::store::{create_run, read_events, resource_held};
    let base = std::env::temp_dir().join(format!("o-nested-runner-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let bin = base.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let source = base.join("fake.rs");
    std::fs::write(&source, r##"fn main() {
        std::fs::write(std::env::var_os("O_FAKE_MARKER").unwrap(), "ok").unwrap();
        println!(r#"{{"type":"result","subtype":"success","is_error":false,"result":"ok"}}"#);
    }"##).unwrap();
    assert!(Command::new("rustc").arg(&source).arg("-o").arg(bin.join("claude.exe")).creation_flags(CREATE_NO_WINDOW).status().unwrap().success());
    let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()))).unwrap();
    for limited in [false, true] {
        let root = base.join(if limited { "limited" } else { "allowed" });
        let project = root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let home = root.join("local").join("project_O");
        let card = Card { id: "card".into(), provider: ProviderId::Claude, role: None, task: "test".into(), model: None, effort: None, depends_on: vec![], resources: vec![Resource { kind: ResourceKind::External, key: "test".into(), mode: ResourceMode::Write }] };
        let plan = Plan { title: "test".into(), project: project.to_string_lossy().into_owned(), cards: vec![card] };
        create_run(&home, "run", &plan).unwrap();
        let marker = project.join("ran");
        let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        assert!(!raw.is_null());
        let outer = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_ACTIVE_PROCESS | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        limits.BasicLimitInformation.ActiveProcessLimit = if limited { 2 } else { 3 };
        assert_ne!(unsafe { SetInformationJobObject(raw, JobObjectExtendedLimitInformation, (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(), std::mem::size_of_val(&limits) as u32) }, 0);
        let mut runner = Command::new(env!("CARGO_BIN_EXE_o")).args(["_run", "run"]).env("LOCALAPPDATA", root.join("local"))
            .env("PATH", &path).env("O_FAKE_MARKER", &marker).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED).spawn().unwrap();
        assert_ne!(unsafe { AssignProcessToJobObject(outer.as_raw_handle(), runner.as_raw_handle()) }, 0);
        o_core::sys::proc::resume_worker(runner.id()).unwrap();
        assert!(runner.wait().unwrap().success());
        let expected = if limited { CardStatus::Failed } else { CardStatus::Done };
        assert!(read_events(&home, "run").iter().any(|event| matches!(event, OEvent::CardEnd { status, .. } if *status == expected)));
        assert_eq!(marker.exists(), !limited);
        assert!(!resource_held(&home, "run", "card").unwrap());
    }
}

#[cfg(windows)]
#[test]
fn actual_cli_lead_normal_crash_stop_and_marker_failure() {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    struct Owner(std::process::Child);
    impl Drop for Owner { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }
    let base = std::env::temp_dir().join(format!("o-lead-runner-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let bin = base.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let source = base.join("fake.rs");
    std::fs::write(&source, r##"fn main() {
        if std::env::var_os("ANTHROPIC_API_KEY").is_some() { std::process::exit(9); }
        std::fs::write(std::env::var_os("O_FAKE_MARKER").unwrap(), std::process::id().to_string()).unwrap();
        if std::env::var_os("O_FAKE_BLOCK").is_some() { std::thread::sleep(std::time::Duration::from_secs(30)); }
        println!(r#"{{"type":"result","subtype":"success","is_error":false,"result":"fake-lead-ok"}}"#);
    }"##).unwrap();
    assert!(Command::new("rustc").arg(&source).arg("-o").arg(bin.join("claude.exe")).creation_flags(0x08000000).status().unwrap().success());
    let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()))).unwrap();
    for mode in ["normal", "crash", "stop", "marker-fail"] {
        let root = base.join(mode);
        let project = root.join("project");
        let home = root.join("local").join("project_O");
        std::fs::create_dir_all(&project).unwrap();
        let marker = project.join("pid");
        if mode == "marker-fail" {
            let thread = o_core::lead::thread_for(&home, &project.to_string_lossy(), false).unwrap();
            std::fs::create_dir(o_core::lead::thread_dir(&home, &thread).join("running.json")).unwrap();
        }
        let mut command = Command::new(env!("CARGO_BIN_EXE_o"));
        command.args(["lead", "send", "--profile", "own", "--manual-dispatch", "--project"]).arg(&project).arg("hello")
            .env("LOCALAPPDATA", root.join("local")).env("PATH", &path).env("O_FAKE_MARKER", &marker).env("ANTHROPIC_API_KEY", "fake-only")
            .env_remove("O_AGENT_ROLE").env_remove("O_CARD_ID").stdin(Stdio::null()).stderr(Stdio::null()).creation_flags(0x08000000);
        if mode == "crash" || mode == "stop" { command.env("O_FAKE_BLOCK", "1").stdout(Stdio::null()); } else { command.env_remove("O_FAKE_BLOCK").stdout(Stdio::piped()); }
        let mut owner = Owner(command.spawn().unwrap());
        if mode == "crash" || mode == "stop" {
            let deadline = Instant::now() + Duration::from_secs(15);
            while !marker.exists() { assert!(Instant::now() < deadline, "fake lead did not start"); std::thread::sleep(Duration::from_millis(20)); }
            let pid: u32 = std::fs::read_to_string(&marker).unwrap().parse().unwrap();
            let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
            assert!(!raw.is_null());
            let lead = unsafe { OwnedHandle::from_raw_handle(raw) };
            if mode == "crash" { owner.0.kill().unwrap(); }
            else {
                let output = Command::new(env!("CARGO_BIN_EXE_o")).args(["lead", "stop", "--project"]).arg(&project)
                    .env("LOCALAPPDATA", root.join("local")).env_remove("O_AGENT_ROLE").env_remove("O_CARD_ID")
                    .creation_flags(0x08000000).output().unwrap();
                assert!(output.status.success());
            }
            owner.0.wait().unwrap();
            assert_eq!(unsafe { WaitForSingleObject(lead.as_raw_handle(), 10_000) }, WAIT_OBJECT_0);
            if mode == "crash" {
                assert_eq!(o_core::lead::show(&home, Some(&project.to_string_lossy()))["thread"]["busy"], "unknown");
                assert!(o_core::lead::thread_for(&home, &project.to_string_lossy(), true).is_err());
            } else {
                assert!(o_core::lead::show(&home, Some(&project.to_string_lossy()))["thread"]["busy"].is_null());
                assert!(o_core::lead::thread_for(&home, &project.to_string_lossy(), true).is_ok());
            }
        } else if mode == "marker-fail" {
            assert!(!owner.0.wait().unwrap().success());
            assert!(!marker.exists());
        } else {
            use std::io::Read;
            let mut output = String::new();
            owner.0.stdout.take().unwrap().read_to_string(&mut output).unwrap();
            assert!(owner.0.wait().unwrap().success());
            assert!(marker.exists());
            assert!(output.contains("fake-lead-ok"), "fake lead reply was missing");
        }
    }
}
