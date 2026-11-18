//! The real Windows runner: `o dispatch` freezes the plan and hands it to `o _run-approved`
//! through a pipe that only a dispatching parent can make, and every card gets its own handle
//! owner and job. These tests used to start `o _run` themselves; that entry now refuses input
//! that did not come from such a parent (ADR-0049), so they start runs the way the app does.
#![cfg(windows)]

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use o_core::run::events::{CardStatus, OEvent};
use o_core::run::store::{list_runs, read_events, resource_held};

/// `CREATE_NO_WINDOW`: nothing these tests start opens a console on CI.
const NO_WINDOW: u32 = 0x08000000;
/// A run on a loaded CI machine: isolate the project, start the fake CLI, settle.
const RUN_LIMIT: Duration = Duration::from_secs(90);

fn temp(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("{name}-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&base).unwrap();
    base
}

/// The state folder `o` uses when `LOCALAPPDATA` is this test's.
fn state_home(base: &Path) -> PathBuf {
    base.join("local").join("project_O")
}

/// A project folder with one file, so the runner has something to copy for its isolated card.
fn new_project(root: &Path) -> PathBuf {
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("source.txt"), "source").unwrap();
    project
}

/// A stand-in provider CLI built with `rustc`, so the real runner starts a real process.
fn fake_cli(base: &Path, name: &str, source: &str) -> PathBuf {
    let bin = base.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let file = base.join(format!("{name}.rs"));
    std::fs::write(&file, source).unwrap();
    assert!(Command::new("rustc").arg(&file).arg("-o").arg(bin.join(format!("{name}.exe"))).creation_flags(NO_WINDOW).status().unwrap().success());
    bin
}

/// The plan on disk for `o dispatch`, which checks it and freezes it for the runner.
fn plan(base: &Path, project: &Path, cards: serde_json::Value) -> PathBuf {
    let path = base.join("plan.json");
    std::fs::write(&path, serde_json::json!({ "title": "test", "project": project.to_string_lossy(), "cards": cards }).to_string()).unwrap();
    path
}

/// `o dispatch`: the way the app starts a run, with this test's state folder and fake CLI first on PATH.
fn dispatch(base: &Path, plan: &Path, bin: &Path) -> Command {
    let path = std::env::join_paths(std::iter::once(bin.to_path_buf()).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()))).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_o"));
    command.arg("dispatch").arg(plan).args(["--profile", "own", "--no-context", "--mode", "normal"])
        .env("LOCALAPPDATA", base.join("local")).env("PATH", path)
        .env_remove("O_AGENT_ROLE").env_remove("O_CARD_ID").creation_flags(NO_WINDOW);
    command
}

/// The run id `o dispatch` printed, or why it refused the plan.
fn started(output: &Output) -> String {
    assert!(output.status.success(), "dispatch failed: {}", String::from_utf8_lossy(&output.stderr));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("dispatch output: {}", String::from_utf8_lossy(&output.stdout)));
    value["runId"].as_str().expect("dispatch prints a run id").to_string()
}

fn wait_end(home: &Path, run: &str, limit: Duration) -> CardStatus {
    let deadline = Instant::now() + limit;
    loop {
        let events = read_events(home, run);
        if let Some(status) = events.iter().find_map(|event| if let OEvent::RunEnd { status, .. } = event { Some(*status) } else { None }) {
            return status;
        }
        assert!(Instant::now() < deadline, "run {run} did not end: {events:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn ended(home: &Path, run: &str, wanted: CardStatus) {
    let events = read_events(home, run);
    assert!(events.iter().any(|event| matches!(event, OEvent::CardEnd { card_id, status, .. } if card_id == "card" && *status == wanted)), "{events:?}");
}

#[test]
fn cli_handle_owner_signals_readiness_and_wrong_entry_fails() {
    let executable = std::path::PathBuf::from(env!("CARGO_BIN_EXE_o"));
    assert!(o_core::sys::win_launch::HandleOwner::start(executable.clone(), "--o-handle-owner").is_ok());
    assert!(o_core::sys::win_launch::HandleOwner::start(executable, "--bad-handle-owner-entry").is_err());
}

#[test]
fn card_fixture() {
    if let Some(marker) = std::env::var_os("O_FAKE_OWNER_MARKER") {
        std::fs::write(marker, "ran").unwrap();
        println!(r#"{{"type":"result","subtype":"success","is_error":false,"result":"ok"}}"#);
    }
}

#[test]
fn card_uses_cli_handle_owner_and_rejects_wrong_helper() {
    use std::sync::Arc;
    use o_core::run::plan::{Card, Plan, ProviderId, Resource, ResourceKind, ResourceMode};
    use o_core::run::runner::{run_plan, Spawner};
    use o_core::run::store::create_run;
    for valid in [true, false] {
        let base = temp(&format!("o-owner-card-{valid}"));
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

#[test]
fn actual_cli_runner_uses_its_own_helper_for_fake_provider() {
    let base = temp("o-real-runner");
    let project = new_project(&base);
    let bin = fake_cli(&base, "claude", r##"fn main() {
        if std::env::var_os("ANTHROPIC_API_KEY").as_deref() != Some(std::ffi::OsStr::new("fake-only")) { std::process::exit(9); }
        std::fs::write(std::env::var_os("O_FAKE_MARKER").unwrap(), "ok").unwrap();
        println!(r#"{{"type":"result","subtype":"success","is_error":false,"result":"ok"}}"#);
    }"##);
    let path = plan(&base, &project, serde_json::json!([{ "id": "card", "provider": "claude", "task": "test", "resources": [{ "kind": "external", "key": "test", "mode": "write" }] }]));
    let marker = project.join("ran");
    let output = dispatch(&base, &path, &bin).env("O_FAKE_MARKER", &marker).env("ANTHROPIC_API_KEY", "fake-only").output().unwrap();
    let home = state_home(&base);
    let run = started(&output);
    assert_eq!(wait_end(&home, &run, RUN_LIMIT), CardStatus::Done);
    assert!(marker.exists(), "fake CLI was not started");
    ended(&home, &run, CardStatus::Done);
    assert!(!resource_held(&home, &run, "card").unwrap());
}

#[test]
fn opencode_own_profile_passes_environment_through_auth_and_card() {
    let base = temp("o-opencode-env");
    let project = new_project(&base);
    let bin = fake_cli(&base, "opencode", r##"fn main() {
        if std::env::var_os("O_CUSTOM_PROVIDER_API_KEY").as_deref() != Some(std::ffi::OsStr::new("fake-only")) { std::process::exit(9); }
        if std::env::args().nth(1).as_deref() == Some("auth") {
            std::fs::write(std::env::var_os("O_FAKE_AUTH_MARKER").unwrap(), "ok").unwrap();
            println!("[]");
        } else {
            std::fs::write(std::env::var_os("O_FAKE_CARD_MARKER").unwrap(), "ok").unwrap();
            println!(r#"{{"type":"text","part":{{"text":"ok"}}}}"#);
        }
    }"##);
    let path = plan(&base, &project, serde_json::json!([{ "id": "card", "provider": "opencode", "task": "test" }]));
    let auth_marker = base.join("auth");
    let card_marker = base.join("card");
    let output = dispatch(&base, &path, &bin)
        .env("O_CUSTOM_PROVIDER_API_KEY", "fake-only").env("O_FAKE_AUTH_MARKER", &auth_marker).env("O_FAKE_CARD_MARKER", &card_marker)
        .output().unwrap();
    let home = state_home(&base);
    let run = started(&output);
    assert_eq!(wait_end(&home, &run, RUN_LIMIT), CardStatus::Done);
    assert!(auth_marker.exists() && card_marker.exists(), "the own profile kept the fake's environment");
    ended(&home, &run, CardStatus::Done);
}

#[test]
fn killed_actual_runner_stops_worker_and_preserves_reservation() {
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE};
    let base = temp("o-killed-runner");
    let project = new_project(&base);
    let bin = fake_cli(&base, "claude", r#"fn main() {
        std::fs::write(std::env::var_os("O_FAKE_MARKER").unwrap(), std::process::id().to_string()).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(30));
    }"#);
    let path = plan(&base, &project, serde_json::json!([{ "id": "card", "provider": "claude", "task": "test", "resources": [{ "kind": "external", "key": "test", "mode": "write" }] }]));
    let marker = project.join("pid");
    let mut dispatcher = dispatch(&base, &path, &bin).env("O_FAKE_MARKER", &marker)
        .env_remove("ANTHROPIC_API_KEY").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while !marker.exists() { assert!(Instant::now() < deadline, "fake worker did not start"); std::thread::sleep(Duration::from_millis(20)); }
    let home = state_home(&base);
    let run = list_runs(&home).pop().expect("dispatch created a run");
    let worker_pid: u32 = std::fs::read_to_string(&marker).unwrap().trim().parse().unwrap();
    let raw_worker = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, worker_pid) };
    assert!(!raw_worker.is_null(), "fake worker disappeared");
    let worker = unsafe { OwnedHandle::from_raw_handle(raw_worker) };
    let mark: serde_json::Value = serde_json::from_slice(&std::fs::read(o_core::run::store::run_dir(&home, &run).join("runner.json")).unwrap()).unwrap();
    let runner_pid = u32::try_from(mark["pid"].as_u64().unwrap()).unwrap();
    let raw_runner = unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, 0, runner_pid) };
    assert!(!raw_runner.is_null(), "runner disappeared");
    let runner = unsafe { OwnedHandle::from_raw_handle(raw_runner) };
    assert_ne!(unsafe { TerminateProcess(runner.as_raw_handle(), 1) }, 0);
    let _ = dispatcher.wait();
    assert_eq!(unsafe { WaitForSingleObject(runner.as_raw_handle(), 10_000) }, WAIT_OBJECT_0);
    assert_eq!(unsafe { WaitForSingleObject(worker.as_raw_handle(), 10_000) }, WAIT_OBJECT_0, "the worker outlived its runner");
    assert!(!read_events(&home, &run).iter().any(|event| matches!(event, OEvent::RunEnd { .. })), "a killed runner closed its run");
    assert!(resource_held(&home, &run, "card").unwrap());
}

#[test]
fn actual_card_respects_nested_job_process_limit() {
    use windows_sys::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE};
    use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};
    let base = temp("o-nested-runner");
    let bin = fake_cli(&base, "claude", r##"fn main() {
        std::fs::write(std::env::var_os("O_FAKE_MARKER").unwrap(), "ok").unwrap();
        println!(r#"{{"type":"result","subtype":"success","is_error":false,"result":"ok"}}"#);
    }"##);
    for limited in [false, true] {
        let root = base.join(if limited { "limited" } else { "allowed" });
        let project = new_project(&root);
        let path = plan(&root, &project, serde_json::json!([{ "id": "card", "provider": "claude", "task": "test", "resources": [{ "kind": "external", "key": "test", "mode": "write" }] }]));
        let marker = project.join("ran");
        // Suspended, so the job is in place before the dispatcher starts the runner; the runner
        // and anything it starts stay in this job, which is the app's own process limit.
        let dispatcher = dispatch(&root, &path, &bin).env("O_FAKE_MARKER", &marker)
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED).spawn().unwrap();
        let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        assert!(!raw.is_null());
        let outer = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_ACTIVE_PROCESS | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // Three processes start the card: the runner, its handle owner and the worker. The
        // dispatcher may still be on its way out, so room for it is part of the allowed limit.
        limits.BasicLimitInformation.ActiveProcessLimit = if limited { 2 } else { 4 };
        assert_ne!(unsafe { SetInformationJobObject(outer.as_raw_handle(), JobObjectExtendedLimitInformation, (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(), std::mem::size_of_val(&limits) as u32) }, 0);
        assert_ne!(unsafe { AssignProcessToJobObject(outer.as_raw_handle(), dispatcher.as_raw_handle()) }, 0);
        o_core::sys::proc::resume_worker(dispatcher.id()).unwrap();
        let output = dispatcher.wait_with_output().unwrap();
        let home = state_home(&root);
        let run = started(&output);
        let expected = if limited { CardStatus::Failed } else { CardStatus::Done };
        assert_eq!(wait_end(&home, &run, RUN_LIMIT), expected);
        ended(&home, &run, expected);
        assert_eq!(marker.exists(), !limited);
        assert!(!resource_held(&home, &run, "card").unwrap());
    }
}
