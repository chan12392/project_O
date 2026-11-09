//! Runs only isolated fake children and temporary state. Also usable in an independent crate.
#![cfg(target_os = "macos")]
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use chrono::Utc;
use o_core::run::plan::{Card, ProviderId, Resource, ResourceKind, ResourceMode};
use o_core::run::store::{mark_spawning, recover_resource, reserve_slot, resource_leases, Reserved};

fn fixture() -> (PathBuf, PathBuf) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("o-mac-resource-{}-{nonce}-{sequence}", std::process::id()));
    let project = base.join("project");
    std::fs::create_dir_all(&project).unwrap();
    (base.join("state"), project)
}

fn card(key: &str) -> Card {
    Card { id: "card".into(), provider: ProviderId::Claude, role: None, task: "test".into(), model: None, effort: None, depends_on: vec![], resources: vec![Resource { kind: ResourceKind::ProjectPath, key: key.into(), mode: ResourceMode::Write }] }
}

fn reserve(home: &Path, project: &Path, run: &str, key: &str) -> Reserved {
    reserve_slot(home, run, &card(key), project.to_str().unwrap(), Utc::now()).unwrap()
}

fn aliases(first: &str, second: &str) {
    let (home, project) = fixture();
    std::fs::write(project.join(first), "probe").unwrap();
    let same_file = project.join(second).exists();
    assert!(matches!(reserve(&home, &project, "one", &format!("missing/{first}")), Reserved::Granted(_)));
    let next = reserve(&home, &project, "two", &format!("missing/{second}"));
    if same_file {
        assert!(matches!(next, Reserved::Busy(_)), "same-volume alias was reserved twice: {first:?} {second:?}");
    } else {
        assert!(matches!(next, Reserved::Granted(_)), "distinct names were blocked");
    }
}

#[test]
fn macos_case_alias_reservation() { aliases("CaseName", "casename"); }

#[test]
fn macos_unicode_alias_reservation() { aliases("\u{e9}", "e\u{301}"); }

#[test]
fn fake_owner() {
    let Ok(home) = std::env::var("O_PROBE_HOME") else { return };
    let project = PathBuf::from(std::env::var("O_PROBE_PROJECT").unwrap());
    let Reserved::Granted(reservation) = reserve(Path::new(&home), &project, "owner", "shared") else { panic!("reservation not granted") };
    if std::env::var_os("O_PROBE_SPAWN_INTENT").is_some() { mark_spawning(Path::new(&home), &reservation).unwrap(); }
    std::fs::write(project.join("owner-ready"), "ready").unwrap();
    std::thread::sleep(Duration::from_secs(4));
}

fn owner_recovery(spawn_intent: bool) {
    let (home, project) = fixture();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", "fake_owner", "--nocapture"]).env_clear()
        .env("O_PROBE_HOME", &home).env("O_PROBE_PROJECT", &project)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if spawn_intent { command.env("O_PROBE_SPAWN_INTENT", "1"); }
    let mut child = command.spawn().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while !project.join("owner-ready").exists() && std::time::Instant::now() < deadline {
        assert!(child.try_wait().unwrap().is_none(), "fixture exited before reservation");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(project.join("owner-ready").exists(), "fixture did not become ready");
    let leases = resource_leases(&home).unwrap();
    let lease = &leases[0];
    assert_eq!(lease.owner_pid, Some(child.id()));
    assert_eq!(lease.spawn_intent, Some(spawn_intent));
    assert!(lease.worker_pid.is_none() && lease.exit_confirmed != Some(true));
    let id = lease.id.parse().unwrap();
    assert!(child.try_wait().unwrap().is_none());
    assert!(recover_resource(&home, id).is_err(), "live owner's reservation was released");
    assert!(child.wait().unwrap().success());
    let leases = resource_leases(&home).unwrap();
    let lease = &leases[0];
    let owner_confirmed = lease.owner_started.as_ref().and_then(|value| value.parse().ok())
        .is_some_and(|started| o_core::sys::proc::definitely_exited(child.id(), started));
    #[cfg(windows)]
    assert!(owner_confirmed, "Windows fixture must exercise confirmed owner exit");
    #[cfg(target_os = "macos")]
    if !spawn_intent { assert!(owner_confirmed, "owner exit is unconfirmed; recovery was not verified"); }
    if !spawn_intent && owner_confirmed {
        assert_eq!(lease.state, "recoverable");
        recover_resource(&home, id).expect("proven unstarted reservation should recover");
        assert!(matches!(reserve(&home, &project, "next", "shared"), Reserved::Granted(_)));
    } else {
        assert_eq!(lease.state, "unknown");
        assert!(recover_resource(&home, id).is_err(), "reservation without sufficient evidence was recovered");
        assert!(matches!(reserve(&home, &project, "next", "shared"), Reserved::Busy(_)));
    }
}

#[test]
fn macos_ambiguous_spawn_recovery() { owner_recovery(true); }

#[test]
fn macos_unstarted_owner_recovery() { owner_recovery(false); }

#[cfg(target_os = "macos")]
#[test]
fn fake_card() {
    let Ok(ready) = std::env::var("O_PROBE_CARD") else { return };
    if std::env::var_os("O_PROBE_ESCAPE").is_some() {
        spawn_escaped(Path::new(&ready).parent().unwrap(), std::env::var_os("O_PROBE_HOLD").is_some());
    }
    std::fs::write(ready, "ready").unwrap();
    if std::env::var_os("O_PROBE_STOP").is_some() { std::thread::sleep(Duration::from_secs(5)); }
    println!(r#"{{"type":"result","subtype":"success","is_error":false,"result":"ok"}}"#);
}

#[cfg(target_os = "macos")]
fn card_run(stop: bool) {
    use o_core::run::events::CardStatus;
    use o_core::run::plan::Plan;
    use o_core::run::runner::{run_plan, stop_run, Spawner};
    use o_core::run::store::{create_run, resource_held};
    use std::sync::{mpsc, Arc};
    let (home, project) = fixture();
    let plan = Plan { title: "probe".into(), project: project.to_string_lossy().into_owned(), cards: vec![card("shared")] };
    create_run(&home, "run", &plan).unwrap();
    let child_home = home.clone();
    let ready = project.join("ready");
    let child_ready = ready.clone();
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let spawner: Arc<Spawner> = Arc::new(move |_, _| {
            let mut launch = o_core::sys::launch::Launch::new(std::env::current_exe()?);
            launch.args(["--exact", "fake_card", "--nocapture"]).env_clear().env("O_PROBE_CARD", &child_ready);
            if stop { launch.env("O_PROBE_STOP", "1"); }
            Ok(launch)
        });
        let _ = send.send(run_plan(&child_home, "run", &plan, None, spawner));
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !ready.exists() && std::time::Instant::now() < deadline { std::thread::sleep(Duration::from_millis(20)); }
    assert!(ready.exists(), "fake card did not start");
    let started = std::time::Instant::now();
    if stop { assert!(stop_run(&home, "run").unwrap()); }
    let status = receive.recv_timeout(Duration::from_secs(12)).expect("runner did not finish").unwrap();
    if stop { assert!(started.elapsed() < Duration::from_secs(4), "stop did not promptly end the owned child"); }
    assert_eq!(status, if stop { CardStatus::Stopped } else { CardStatus::Done });
    assert!(!resource_held(&home, "run", "card").unwrap(), "successful termination left a reservation");
}

#[cfg(target_os = "macos")]
#[test]
fn macos_card_normal() { card_run(false); }

#[cfg(target_os = "macos")]
#[test]
fn macos_card_stop() { card_run(true); }

#[cfg(target_os = "macos")]
fn spawn_escaped(base: &Path, hold_pipe: bool) {
    use std::os::unix::process::CommandExt;
    unsafe extern "C" { fn setsid() -> i32; }
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", "fake_lead", "--nocapture"]).env_clear()
        .env("O_PROBE_LEAD", "grandchild").env("O_PROBE_LEAD_DIR", base)
        .stdin(Stdio::null()).stdout(if hold_pipe { Stdio::inherit() } else { Stdio::null() }).stderr(Stdio::null());
    unsafe { command.pre_exec(|| if setsid() < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }); }
    // This fixture must exit before its detached child; launchd reaps the child.
    #[allow(clippy::zombie_processes)]
    let _grandchild = command.spawn().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while !base.join("grand-ready").exists() && std::time::Instant::now() < deadline { std::thread::sleep(Duration::from_millis(20)); }
    assert!(base.join("grand-ready").exists());
}

#[cfg(target_os = "macos")]
fn card_escaped(hold_pipe: bool) {
    use o_core::run::events::CardStatus;
    use o_core::run::plan::Plan;
    use o_core::run::runner::{run_plan, Spawner};
    use o_core::run::store::{create_run, resource_held};
    use std::sync::{mpsc, Arc};
    let (home, project) = fixture();
    let plan = Plan { title: "probe".into(), project: project.to_string_lossy().into_owned(), cards: vec![card("shared")] };
    create_run(&home, "run", &plan).unwrap();
    let child_home = home.clone();
    let ready = project.join("ready");
    let child_ready = ready.clone();
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let spawner: Arc<Spawner> = Arc::new(move |_, _| {
            let mut launch = o_core::sys::launch::Launch::new(std::env::current_exe()?);
            launch.args(["--exact", "fake_card", "--nocapture"]).env_clear().env("O_PROBE_CARD", &child_ready).env("O_PROBE_ESCAPE", "1");
            if hold_pipe { launch.env("O_PROBE_HOLD", "1"); }
            Ok(launch)
        });
        let _ = send.send(run_plan(&child_home, "run", &plan, None, spawner));
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !ready.exists() && std::time::Instant::now() < deadline { std::thread::sleep(Duration::from_millis(20)); }
    assert!(ready.exists(), "fake card did not start");
    let status = receive.recv_timeout(Duration::from_secs(12)).expect("runner did not finish").unwrap();
    assert_eq!(status, CardStatus::Done);
    assert_descendant_exited(&project);
    assert!(!resource_held(&home, "run", "card").unwrap(), "proven exit left a reservation");
    assert!(matches!(reserve(&home, &project, "next", "shared"), Reserved::Granted(_)));

}

#[cfg(target_os = "macos")]
#[test]
fn macos_card_escaped_closed() { card_escaped(false); }

#[cfg(target_os = "macos")]
#[test]
fn macos_card_escaped_pipe() { card_escaped(true); }

#[cfg(target_os = "macos")]
#[test]
fn fake_lead() {
    let Ok(mode) = std::env::var("O_PROBE_LEAD") else { return };
    let base = PathBuf::from(std::env::var_os("O_PROBE_LEAD_DIR").unwrap());
    if mode == "grandchild" {
        let pid = std::process::id();
        let started = o_core::sys::proc::started_at(pid).unwrap();
        std::fs::write(base.join("grand-ready"), format!("{pid} {started}")).unwrap();
        std::thread::sleep(Duration::from_secs(5));
        std::fs::write(base.join("grand-finished"), "done").unwrap();
        return;
    }
    if mode == "escape" {
        spawn_escaped(&base, false);
    }
    std::fs::write(base.join("lead-ready"), "ready").unwrap();
    if mode == "stop" { std::thread::sleep(Duration::from_secs(5)); }
    println!(r#"{{"type":"result","subtype":"success","is_error":false,"result":"ok"}}"#);
}

#[cfg(target_os = "macos")]
fn lead_run(mode: &'static str) {
    use o_core::lead::{self, SendRequest};
    let (home, project) = fixture();
    let worker_home = home.clone();
    let worker_project = project.clone();
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let directory = worker_project.clone();
        let spawner = move |_: &o_core::providers::LaunchSpec, _: &str| {
            let mut launch = o_core::sys::launch::Launch::new(std::env::current_exe()?);
            launch.args(["--exact", "fake_lead", "--nocapture"]).env_clear()
                .env("O_PROBE_LEAD", mode).env("O_PROBE_LEAD_DIR", &directory);
            Ok(launch)
        };
        let project = worker_project.to_str().unwrap();
        let request = SendRequest { project, thread: None, provider: ProviderId::Claude, model: None, effort: None, text: "test", attach: &[], fresh: false, clean: None, yolo: false };
        let _ = send.send(lead::send(&worker_home, &request, &spawner));
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !project.join("lead-ready").exists() && std::time::Instant::now() < deadline { std::thread::sleep(Duration::from_millis(20)); }
    assert!(project.join("lead-ready").exists(), "fake lead did not start");
    if mode == "stop" { assert!(lead::stop(&home, project.to_str(), None).unwrap()); }
    receive.recv_timeout(Duration::from_secs(12)).expect("lead did not finish").unwrap();
    if mode == "escape" { assert_descendant_exited(&project); }
    assert!(lead::show(&home, project.to_str(), None).unwrap()["thread"]["busy"].is_null(), "completed lead still blocks the next turn");

}

#[cfg(target_os = "macos")]
#[test]
fn macos_lead_normal() { lead_run("normal"); }

#[cfg(target_os = "macos")]
#[test]
fn macos_lead_stop() { lead_run("stop"); }

#[cfg(target_os = "macos")]
#[test]
fn macos_lead_escaped_descendant() { lead_run("escape"); }

fn assert_descendant_exited(project: &Path) {
    let identity = std::fs::read_to_string(project.join("grand-ready")).unwrap();
    let mut values = identity.split_whitespace();
    let pid = values.next().unwrap().parse().unwrap();
    let started = values.next().unwrap().parse().unwrap();
    assert!(o_core::sys::proc::definitely_exited(pid, started), "escaped child still exists after completion");
    assert!(!project.join("grand-finished").exists(), "fixture ended naturally; forced cleanup was not exercised");
}
