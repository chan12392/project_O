use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::Utc;
use o_core::run::plan::{Card, ProviderId, Resource, ResourceKind, ResourceMode};
use o_core::run::store::{link_slot, release_slot, reserve_slot, resource_held, Reservation, Reserved};

fn fake_worker_launch() -> std::io::Result<o_core::sys::launch::Launch> {
    let launch = o_core::sys::launch::Launch::new(std::env::current_exe()?);
    #[cfg(windows)]
    { let mut launch = launch; launch.handle_owner_executable = Some(PathBuf::from(env!("CARGO_BIN_EXE_o-handle-owner"))); Ok(launch) }
    #[cfg(not(windows))]
    { Ok(launch) }
}

fn fixture() -> (PathBuf, PathBuf) {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("o-resource-{}-{unique}", std::process::id()));
    let project = base.join("project");
    std::fs::create_dir_all(&project).unwrap();
    (base.join("state"), project)
}

#[test]
#[cfg(target_os = "linux")]
fn linux_process_identity_distinguishes_live_reused_and_reaped_pids() {
    use o_core::sys::proc::{definitely_exited, started_at};
    let pid = std::process::id();
    let started = started_at(pid).unwrap();
    assert!(!definitely_exited(pid, started));
    assert!(definitely_exited(pid, started + 1));
    assert!(!definitely_exited(0, 0));
    assert!(!definitely_exited(u32::MAX, 0));
    let mut child = std::process::Command::new("sh").args(["-c", "exit 0"]).spawn().unwrap();
    let child_pid = child.id();
    child.wait().unwrap();
    assert!(definitely_exited(child_pid, 0));
}

fn card(id: &str, resources: Vec<Resource>) -> Card {
    Card { id: id.into(), provider: ProviderId::Claude, role: None, task: "test".into(), model: None, effort: None, depends_on: vec![], resources }
}

fn res(kind: ResourceKind, key: &str, mode: ResourceMode) -> Resource {
    Resource { kind, key: key.into(), mode }
}

fn reserve(home: &Path, project: &Path, run: &str, card: &Card) -> Reserved {
    reserve_slot(home, run, card, project.to_str().unwrap(), Utc::now()).unwrap()
}

fn granted(value: Reserved) -> Reservation {
    match value { Reserved::Granted(lease) => lease, _ => panic!("reservation not granted") }
}

#[test]
fn paths_external_keys_and_unknown_declarations_conflict_safely() {
    use ResourceKind::{External, ProjectPath};
    use ResourceMode::{Read, Write};
    let (home, project) = fixture();
    std::fs::create_dir(project.join("src")).unwrap();
    let first = granted(reserve(&home, &project, "one", &card("one", vec![res(ProjectPath, "src/./file", Read)])));
    let second = granted(reserve(&home, &project, "two", &card("two", vec![res(ProjectPath, "src", Read)])));
    assert!(matches!(reserve(&home, &project, "three", &card("three", vec![res(ProjectPath, "src", Write)])), Reserved::Busy(_)));
    assert!(matches!(reserve(&home, &project, "unknown", &card("unknown", vec![])), Reserved::Busy(_)));
    release_slot(&home, &first, false).unwrap();
    release_slot(&home, &second, false).unwrap();
    let ext = granted(reserve(&home, &project, "ext", &card("ext", vec![res(External, " Service:Shared ", Write)])));
    assert!(matches!(reserve(&home, &project, "other", &card("other", vec![res(External, "service:shared", Read)])), Reserved::Busy(_)));
    release_slot(&home, &ext, false).unwrap();
    assert!(reserve_slot(&home, "invalid", &card("invalid", vec![res(ProjectPath, "../outside", Write)]), project.to_str().unwrap(), Utc::now()).is_err());
}

#[test]
fn old_lease_migrates_without_inventing_process_identity() {
    let (home, _) = fixture();
    std::fs::create_dir_all(&home).unwrap();
    let conn = rusqlite::Connection::open(home.join("o.db")).unwrap();
    conn.execute_batch("CREATE TABLE resource_leases (slot_id INTEGER PRIMARY KEY, run_id TEXT NOT NULL, card_id TEXT NOT NULL, resources TEXT NOT NULL);\n\
        INSERT INTO resource_leases VALUES (5, 'old', 'card', '[]'); PRAGMA user_version=4;").unwrap();
    drop(conn);
    let leases = o_core::run::store::resource_leases(&home).unwrap();
    assert_eq!(leases.len(), 1);
    assert_eq!(leases[0].run_id, "old");
    assert!(leases[0].owner_pid.is_none() && leases[0].worker_pid.is_none());
    assert!(leases[0].owner_started.is_none() && leases[0].worker_started.is_none());
    assert!(leases[0].exit_confirmed.is_none());
    assert_eq!(leases[0].state, "unknown");
    assert!(o_core::run::store::recover_resource(&home, 5).is_err());
}

#[test]
fn simultaneous_runs_cannot_both_reserve_a_write() {
    let (home, project) = fixture();
    let barrier = Arc::new(Barrier::new(3));
    let mut threads = Vec::new();
    for id in ["one", "two"] {
        let (home, project, barrier) = (home.clone(), project.clone(), Arc::clone(&barrier));
        threads.push(std::thread::spawn(move || {
            let card = card(id, vec![res(ResourceKind::External, "shared", ResourceMode::Write)]);
            barrier.wait();
            reserve(&home, &project, id, &card)
        }));
    }
    barrier.wait();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| matches!(r, Reserved::Granted(_))).count(), 1);
    assert_eq!(results.iter().filter(|r| matches!(r, Reserved::Busy(_))).count(), 1);
}

#[test]
#[allow(clippy::zombie_processes)] // The fixture deliberately leaves a descendant for the job to stop.
fn process_fixture() {
    if let Some(path) = std::env::var_os("O_RESOURCE_PARALLEL") {
        let path = PathBuf::from(path);
        let id = std::env::var("O_RESOURCE_PARALLEL_ID").unwrap();
        std::fs::write(path.join(&id), "ready").unwrap();
        let until = std::time::Instant::now() + Duration::from_secs(10);
        while !(path.join("one").exists() && path.join("two").exists()) {
            assert!(std::time::Instant::now() < until, "both cards must overlap");
            std::thread::sleep(Duration::from_millis(10));
        }
        println!("{}", serde_json::json!({"type":"result", "subtype":"success", "is_error":false, "result":id}));
        return;
    }
    if let Some(path) = std::env::var_os("O_RESOURCE_AUDIT") {
        let report = serde_json::json!({"api_key_present": std::env::var_os("ANTHROPIC_API_KEY").is_some(), "custom": std::env::var("O_RESOURCE_CUSTOM").ok()});
        std::fs::write(path, report.to_string()).unwrap();
        println!(r#"{{"type":"result","subtype":"success","is_error":false,"result":"ok"}}"#);
        return;
    }
    if let Some(home) = std::env::var_os("O_RESOURCE_HOME") {
        let home = PathBuf::from(home);
        let project = PathBuf::from(std::env::var_os("O_RESOURCE_PROJECT").unwrap());
        let id = std::env::var("O_RESOURCE_ID").unwrap();
        std::fs::write(project.join(format!("{id}.ready")), "").unwrap();
        for _ in 0..500 {
            if project.join("go").exists() { break; }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(project.join("go").exists());
        let result = reserve(&home, &project, &id, &card(&id, vec![res(ResourceKind::External, "shared", ResourceMode::Write)]));
        if std::env::var_os("O_RESOURCE_MARK_SPAWN").is_some() {
            if let Reserved::Granted(lease) = &result { o_core::run::store::mark_spawning(&home, lease).unwrap(); }
        }
        #[cfg(windows)]
        if std::env::var_os("O_RESOURCE_MARK_EXIT").is_some() {
            let Reserved::Granted(lease) = &result else { panic!("lease missing") };
            o_core::run::store::mark_spawning(&home, lease).unwrap();
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command.args(["--exact", "process_fixture"]).env_remove("O_RESOURCE_HOME").env_remove("O_RESOURCE_MARK_EXIT").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
            let mut worker = o_core::sys::proc::spawn_worker(&mut command).unwrap();
            let mut tree = o_core::sys::proc::contain(&worker).unwrap();
            link_slot(&home, lease, worker.id()).unwrap();
            o_core::sys::proc::resume_worker(worker.id()).unwrap();
            worker.wait().unwrap();
            assert!(tree.stop_and_confirm());
            o_core::run::store::mark_resource_exited(&home, lease).unwrap();
        }
        let text = if matches!(result, Reserved::Granted(_)) { "granted" } else { "busy" };
        std::fs::write(project.join(format!("{id}.result")), text).unwrap();
        std::thread::sleep(Duration::from_secs(1));
        if let Reserved::Granted(lease) = result { release_slot(&home, &lease, false).unwrap(); }
    } else if let Some(path) = std::env::var_os("O_RESOURCE_DESCENDANT") {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "process_fixture"]).env_remove("O_RESOURCE_DESCENDANT").env("O_RESOURCE_CHILD_MS", "1500").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        let child = o_core::sys::proc::hide_window(&mut command).spawn().unwrap();
        std::fs::write(path, child.id().to_string()).unwrap();
        println!(r#"{{"type":"result","subtype":"success","is_error":false,"result":"ok"}}"#);
    } else if let Some(path) = std::env::var_os("O_RESOURCE_LOCK") {
        let path = PathBuf::from(path);
        let created = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).is_ok();
        if !created { std::fs::write(path.with_extension("conflict"), "overlap").unwrap(); }
        std::thread::sleep(Duration::from_millis(400));
        if created { std::fs::remove_file(&path).unwrap(); }
        println!(r#"{{"type":"result","subtype":"success","is_error":false,"result":"ok"}}"#);
    } else if let Ok(ms) = std::env::var("O_RESOURCE_CHILD_MS") { std::thread::sleep(Duration::from_millis(ms.parse().unwrap())); }
    else if std::env::var_os("O_RESOURCE_FAKE").is_some() { std::thread::sleep(Duration::from_secs(30)); }
}

#[cfg(windows)]
#[test]
fn independent_cards_run_concurrently_with_separate_outputs() {
    use o_core::run::events::{CardStatus, OEvent};
    use o_core::run::plan::Plan;
    use o_core::run::runner::{run_plan, Spawner};
    use o_core::run::store::{create_run, read_events};
    let (home, project) = fixture();
    let cards = ["one", "two"].into_iter().map(|id| card(id, vec![res(ResourceKind::External, id, ResourceMode::Write)])).collect();
    let plan = Plan { title: "test".into(), project: project.to_string_lossy().into_owned(), cards };
    create_run(&home, "run", &plan).unwrap();
    let directory = project.clone();
    let spawner: Arc<Spawner> = Arc::new(move |spec, _| {
        let mut launch = fake_worker_launch()?;
        launch.args(["--exact", "process_fixture", "--nocapture"]).env("O_RESOURCE_PARALLEL", &directory).env("O_RESOURCE_PARALLEL_ID", &spec.env["O_CARD_ID"]);
        Ok(launch)
    });
    assert_eq!(run_plan(&home, "run", &plan, None, spawner).unwrap(), CardStatus::Done);
    let events = read_events(&home, "run");
    for id in ["one", "two"] {
        assert!(events.iter().any(|event| matches!(event, OEvent::CardEnd { card_id, status: CardStatus::Done, result: Some(text), .. } if card_id == id && text == id)));
        assert!(!resource_held(&home, "run", id).unwrap());
    }
}

#[cfg(windows)]
#[test]
fn card_spawner_respects_explicit_environment_and_removes_fake_api_key() {
    use o_core::run::events::CardStatus;
    use o_core::run::plan::Plan;
    use o_core::run::runner::{run_plan, Spawner};
    use o_core::run::store::create_run;
    for replace in [false, true] {
        let (home, project) = fixture();
        let plan = Plan { title: "test".into(), project: project.to_string_lossy().into_owned(), cards: vec![card("one", vec![res(ResourceKind::External, "audit", ResourceMode::Write)])] };
        create_run(&home, "one", &plan).unwrap();
        let report = project.join("audit.json");
        let output = report.clone();
        let spawner: Arc<Spawner> = Arc::new(move |_, _| {
            let mut launch = fake_worker_launch()?;
            if replace { launch.env_clear(); }
            if let Some(root) = std::env::var_os("SystemRoot") { launch.env("SystemRoot", root); }
            launch.args(["--exact", "process_fixture", "--nocapture"]).env("O_RESOURCE_AUDIT", &output)
                .env("O_RESOURCE_CUSTOM", "fixture").env("ANTHROPIC_API_KEY", "fake-only");
            Ok(launch)
        });
        assert_eq!(run_plan(&home, "one", &plan, None, spawner).unwrap(), CardStatus::Done);
        let audit: serde_json::Value = serde_json::from_slice(&std::fs::read(report).unwrap()).unwrap();
        assert_eq!(audit["api_key_present"], false);
        assert_eq!(audit["custom"], "fixture");
        assert!(!resource_held(&home, "one", "one").unwrap());
    }
}

#[cfg(windows)]
#[test]
fn marker_failure_stops_suspended_card_before_releasing_its_reservation() {
    use o_core::run::events::CardStatus;
    use o_core::run::plan::Plan;
    use o_core::run::runner::{run_plan, Spawner};
    use o_core::run::store::{create_run, started_on};
    let (home, project) = fixture();
    let plan = Plan { title: "test".into(), project: project.to_string_lossy().into_owned(), cards: vec![card("one", vec![res(ResourceKind::External, "marker-failure", ResourceMode::Write)])] };
    create_run(&home, "one", &plan).unwrap();
    std::fs::write(home.join("active"), "not a directory").unwrap();
    let marker = project.join("must-not-run");
    let output = marker.clone();
    let spawner: Arc<Spawner> = Arc::new(move |_, _| {
        let mut launch = fake_worker_launch()?;
        launch.args(["--exact", "process_fixture", "--nocapture"]).env("O_RESOURCE_AUDIT", &output);
        Ok(launch)
    });
    assert_eq!(run_plan(&home, "one", &plan, None, spawner).unwrap(), CardStatus::Failed);
    assert!(!marker.exists());
    assert!(!resource_held(&home, "one", "one").unwrap());
    assert_eq!(started_on(&home, Utc::now()).unwrap(), 1);
}

#[cfg(windows)]
#[test]
fn failed_exit_confirmation_keeps_card_reservation() {
    use o_core::run::events::CardStatus;
    use o_core::run::plan::Plan;
    use o_core::run::runner::{run_plan, Spawner};
    use o_core::run::store::create_run;
    let (home, project) = fixture();
    let card = card("one", vec![res(ResourceKind::External, "exit-confirmation", ResourceMode::Write)]);
    let plan = Plan { title: "test".into(), project: project.to_string_lossy().into_owned(), cards: vec![card.clone()] };
    create_run(&home, "one", &plan).unwrap();
    assert!(!resource_held(&home, "one", "one").unwrap());
    let db = rusqlite::Connection::open(home.join("o.db")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_exit BEFORE UPDATE OF exit_confirmed ON resource_leases WHEN NEW.exit_confirmed=1 BEGIN SELECT RAISE(FAIL,'injected'); END;").unwrap();
    drop(db);
    let marker = project.join("ran");
    let output = marker.clone();
    let spawner: Arc<Spawner> = Arc::new(move |_, _| {
        let mut launch = fake_worker_launch()?;
        launch.args(["--exact", "process_fixture", "--nocapture"]).env("O_RESOURCE_AUDIT", &output);
        Ok(launch)
    });
    assert_eq!(run_plan(&home, "one", &plan, None, spawner).unwrap(), CardStatus::Done);
    assert!(marker.exists());
    assert!(resource_held(&home, "one", "one").unwrap());
    assert!(matches!(reserve(&home, &project, "two", &card), Reserved::Busy(_)));
}

#[cfg(windows)]
#[test]
fn failed_spawn_marker_never_starts_card_and_refunds_reservation() {
    use o_core::run::events::CardStatus;
    use o_core::run::plan::Plan;
    use o_core::run::runner::{run_plan, Spawner};
    use o_core::run::store::{create_run, started_on};
    let (home, project) = fixture();
    let plan = Plan { title: "test".into(), project: project.to_string_lossy().into_owned(), cards: vec![card("one", vec![res(ResourceKind::External, "spawn-marker", ResourceMode::Write)])] };
    create_run(&home, "one", &plan).unwrap();
    assert!(!resource_held(&home, "one", "one").unwrap());
    let db = rusqlite::Connection::open(home.join("o.db")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_spawn BEFORE UPDATE OF spawn_intent ON resource_leases WHEN NEW.spawn_intent=1 BEGIN SELECT RAISE(FAIL,'injected'); END;").unwrap();
    drop(db);
    let marker = project.join("must-not-run");
    let output = marker.clone();
    let spawner: Arc<Spawner> = Arc::new(move |_, _| {
        let mut launch = fake_worker_launch()?;
        launch.args(["--exact", "process_fixture", "--nocapture"]).env("O_RESOURCE_AUDIT", &output);
        Ok(launch)
    });
    assert_eq!(run_plan(&home, "one", &plan, None, spawner).unwrap(), CardStatus::Failed);
    assert!(!marker.exists());
    assert!(!resource_held(&home, "one", "one").unwrap());
    assert_eq!(started_on(&home, Utc::now()).unwrap(), 0);
}

#[cfg(windows)]
#[test]
fn failed_native_process_creation_refunds_card_start() {
    use o_core::run::events::CardStatus;
    use o_core::run::plan::Plan;
    use o_core::run::runner::{run_plan, Spawner};
    use o_core::run::store::{create_run, started_on};
    let (home, project) = fixture();
    let plan = Plan { title: "test".into(), project: project.to_string_lossy().into_owned(), cards: vec![card("one", vec![res(ResourceKind::External, "missing-exe", ResourceMode::Write)])] };
    create_run(&home, "one", &plan).unwrap();
    let executable = project.join("missing.exe");
    let spawner: Arc<Spawner> = Arc::new(move |_, _| {
        let mut launch = fake_worker_launch()?;
        launch.executable = executable.clone();
        Ok(launch)
    });
    assert_eq!(run_plan(&home, "one", &plan, None, spawner).unwrap(), CardStatus::Failed);
    assert!(!resource_held(&home, "one", "one").unwrap());
    assert_eq!(started_on(&home, Utc::now()).unwrap(), 0);
}

#[test]
fn separate_processes_compete_in_one_transaction() {
    let (home, project) = fixture();
    let mut children = Vec::new();
    for id in ["one", "two"] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "process_fixture"]).env("O_RESOURCE_HOME", &home).env("O_RESOURCE_PROJECT", &project).env("O_RESOURCE_ID", id).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        children.push(o_core::sys::proc::hide_window(&mut command).spawn().unwrap());
    }
    for _ in 0..500 {
        if ["one", "two"].iter().all(|id| project.join(format!("{id}.ready")).exists()) { break; }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(["one", "two"].iter().all(|id| project.join(format!("{id}.ready")).exists()));
    std::fs::write(project.join("go"), "").unwrap();
    for mut child in children { assert!(child.wait().unwrap().success()); }
    let mut results: Vec<_> = ["one", "two"].iter().map(|id| std::fs::read_to_string(project.join(format!("{id}.result"))).unwrap()).collect();
    results.sort();
    assert_eq!(results, ["busy", "granted"]);
}

#[test]
fn crashed_owner_leaves_its_reservation_intact() {
    let (home, project) = fixture();
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", "process_fixture"]).env("O_RESOURCE_HOME", &home).env("O_RESOURCE_PROJECT", &project).env("O_RESOURCE_ID", "one").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let mut child = o_core::sys::proc::hide_window(&mut command).spawn().unwrap();
    for _ in 0..500 {
        if project.join("one.ready").exists() { break; }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(project.join("one.ready").exists());
    std::fs::write(project.join("go"), "").unwrap();
    for _ in 0..500 {
        if project.join("one.result").exists() { break; }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(std::fs::read_to_string(project.join("one.result")).unwrap(), "granted");
    let id: i64 = o_core::run::store::resource_leases(&home).unwrap()[0].id.parse().unwrap();
    assert!(o_core::run::store::recover_resource(&home, id).is_err());
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(o_core::run::store::held_by_provider(&home).unwrap().get(&ProviderId::Claude), Some(&1));
    let leases = o_core::run::store::resource_leases(&home).unwrap();
    assert_eq!(leases.len(), 1);
    assert_eq!(leases[0].worker_pid, None);
    assert_eq!(leases[0].owner_pid, Some(child.id()));
    assert!(leases[0].owner_started.is_some());
    assert!(leases[0].worker_started.is_none());
    assert!(resource_held(&home, "one", "one").unwrap());
    assert!(matches!(reserve(&home, &project, "two", &card("two", vec![res(ResourceKind::External, "shared", ResourceMode::Write)])), Reserved::Busy(_)));
    #[cfg(any(windows, target_os = "linux"))]
    {
        assert_eq!(leases[0].state, "recoverable");
        let conn = rusqlite::Connection::open(home.join("o.db")).unwrap();
        conn.execute("UPDATE resource_leases SET worker_pid=123 WHERE slot_id=?1", [id]).unwrap();
        assert!(o_core::run::store::recover_resource(&home, id).is_err());
        conn.execute("UPDATE resource_leases SET worker_pid=NULL WHERE slot_id=?1", [id]).unwrap();
        drop(conn);
        let threads: Vec<_> = (0..2).map(|_| {
            let home = home.clone();
            std::thread::spawn(move || o_core::run::store::recover_resource(&home, id))
        }).collect();
        assert_eq!(threads.into_iter().map(|thread| thread.join().unwrap().is_ok()).filter(|ok| *ok).count(), 1);
        assert!(!resource_held(&home, "one", "one").unwrap());
    }
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn ambiguous_spawn_is_never_recovered_from_owner_death_alone() {
    let (home, project) = fixture();
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", "process_fixture"]).env("O_RESOURCE_HOME", &home).env("O_RESOURCE_PROJECT", &project).env("O_RESOURCE_ID", "one").env("O_RESOURCE_MARK_SPAWN", "1").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let mut child = o_core::sys::proc::hide_window(&mut command).spawn().unwrap();
    for _ in 0..500 { if project.join("one.ready").exists() { break; } std::thread::sleep(Duration::from_millis(10)); }
    std::fs::write(project.join("go"), "").unwrap();
    for _ in 0..500 { if project.join("one.result").exists() { break; } std::thread::sleep(Duration::from_millis(10)); }
    assert_eq!(std::fs::read_to_string(project.join("one.result")).unwrap(), "granted");
    let lease = o_core::run::store::resource_leases(&home).unwrap().pop().unwrap();
    assert_eq!(lease.spawn_intent, Some(true));
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(o_core::run::store::resource_leases(&home).unwrap()[0].state, "unknown");
    assert!(o_core::run::store::recover_resource(&home, lease.id.parse().unwrap()).is_err());
    assert!(resource_held(&home, "one", "one").unwrap());
}

#[cfg(windows)]
#[test]
fn confirmed_exit_is_recovered_after_owner_crash_without_refunding_start() {
    let (home, project) = fixture();
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", "process_fixture"]).env("O_RESOURCE_HOME", &home).env("O_RESOURCE_PROJECT", &project).env("O_RESOURCE_ID", "one").env("O_RESOURCE_MARK_EXIT", "1").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let mut owner = o_core::sys::proc::hide_window(&mut command).spawn().unwrap();
    for _ in 0..500 { if project.join("one.ready").exists() { break; } std::thread::sleep(Duration::from_millis(10)); }
    std::fs::write(project.join("go"), "").unwrap();
    for _ in 0..500 { if project.join("one.result").exists() { break; } std::thread::sleep(Duration::from_millis(10)); }
    assert_eq!(std::fs::read_to_string(project.join("one.result")).unwrap(), "granted");
    let lease = o_core::run::store::resource_leases(&home).unwrap().pop().unwrap();
    assert_eq!(lease.exit_confirmed, Some(true));
    assert_eq!(lease.state, "unknown");
    assert!(o_core::run::store::recover_resource(&home, lease.id.parse().unwrap()).is_err());
    owner.kill().unwrap();
    owner.wait().unwrap();
    assert_eq!(o_core::run::store::resource_leases(&home).unwrap()[0].state, "recoverable");
    o_core::run::store::recover_resource(&home, lease.id.parse().unwrap()).unwrap();
    assert!(!resource_held(&home, "one", "one").unwrap());
    assert_eq!(o_core::run::store::started_on(&home, Utc::now()).unwrap(), 1);
}

#[cfg(windows)]
#[test]
fn process_identity_checks_creation_time() {
    let pid = std::process::id();
    let started = o_core::sys::proc::started_at(pid).unwrap();
    assert!(!o_core::sys::proc::definitely_exited(pid, started));
    assert!(o_core::sys::proc::definitely_exited(pid, started + 1));
}

#[test]
fn dead_child_does_not_erase_lease_without_confirmed_release() {
    let (home, project) = fixture();
    let first = card("one", vec![res(ResourceKind::External, "shared", ResourceMode::Write)]);
    let lease = granted(reserve(&home, &project, "one", &first));
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", "process_fixture"]).env("O_RESOURCE_FAKE", "1");
    let mut child = o_core::sys::proc::hide_window(&mut command).spawn().unwrap();
    link_slot(&home, &lease, child.id()).unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(o_core::run::store::held_by_provider(&home).unwrap().get(&ProviderId::Claude), Some(&1));
    let conn = rusqlite::Connection::open(home.join("o.db")).unwrap();
    conn.execute("DELETE FROM slots WHERE run_id='one'", []).unwrap();
    drop(conn);
    assert_eq!(o_core::run::store::held_by_provider(&home).unwrap().get(&ProviderId::Claude), Some(&1));
    let leases = o_core::run::store::resource_leases(&home).unwrap();
    assert_eq!(leases.len(), 1);
    assert_eq!(leases[0].worker_pid, Some(child.id()));
    assert_eq!(leases[0].owner_pid, Some(std::process::id()));
    assert!(leases[0].owner_started.is_some());
    assert!(leases[0].worker_started.is_some());
    assert!(resource_held(&home, "one", "one").unwrap());
    assert!(matches!(reserve(&home, &project, "two", &first), Reserved::Busy(_)));
    release_slot(&home, &lease, true).unwrap();
    assert!(matches!(reserve(&home, &project, "two", &first), Reserved::Granted(_)));
}

#[test]
fn orphaned_lease_without_provider_blocks_capacity_checks() {
    let (home, project) = fixture();
    let lease = granted(reserve(&home, &project, "one", &card("one", vec![])));
    let conn = rusqlite::Connection::open(home.join("o.db")).unwrap();
    conn.execute("DELETE FROM slots WHERE run_id='one'", []).unwrap();
    conn.execute("DELETE FROM starts WHERE run_id='one'", []).unwrap();
    drop(conn);
    assert!(o_core::run::store::held_by_provider(&home).is_err());
    assert!(reserve_slot(&home, "two", &card("two", vec![]), project.to_str().unwrap(), Utc::now()).is_err());
    release_slot(&home, &lease, true).unwrap();
}

#[test]
fn dead_workers_with_unreleased_leases_still_fill_provider_capacity() {
    let (home, project) = fixture();
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", "process_fixture"]).env("O_RESOURCE_FAKE", "1").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let mut child = o_core::sys::proc::hide_window(&mut command).spawn().unwrap();
    for n in 0..o_core::policy::LIMITS.max_active_per_provider(ProviderId::Claude) {
        let id = format!("run{n}");
        let lease = granted(reserve(&home, &project, &id, &card(&id, vec![res(ResourceKind::External, &id, ResourceMode::Write)])));
        link_slot(&home, &lease, child.id()).unwrap();
    }
    child.kill().unwrap();
    child.wait().unwrap();
    let next = card("next", vec![res(ResourceKind::External, "different", ResourceMode::Write)]);
    assert!(matches!(reserve(&home, &project, "next", &next), Reserved::Busy(reason) if reason.contains("동시 실행")));
}

#[cfg(target_os = "linux")]
#[test]
fn linux_lead_timeout_stops_owned_child_and_preserves_unknown_marker() {
    use o_core::lead::{self, SendRequest};
    let (home, project) = fixture();
    let project = project.to_string_lossy().into_owned();
    let req = SendRequest { project: &project, provider: ProviderId::Claude, model: None, effort: None, text: "test", attach: &[], fresh: false, clean: None, yolo: false };
    let spawner = |_: &o_core::providers::LaunchSpec, _: &str| {
        let mut command = fake_worker_launch()?;
        command.args(["--exact", "process_fixture"]).env("O_RESOURCE_FAKE", "1");
        Ok(command)
    };
    let started = std::time::Instant::now();
    let events = lead::send_within(&home, &req, &spawner, Duration::from_secs(1)).unwrap();
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(events.iter().any(|event| matches!(event, lead::LeadEvent::Error { text, .. } if text.contains("시간"))));
    assert!(lead::send(&home, &req, &spawner).is_err());
}

#[cfg(windows)]
#[test]
fn windows_lead_can_start_another_turn_after_exit() {
    use o_core::lead::{self, SendRequest};
    let (home, project) = fixture();
    let project_text = project.to_string_lossy().into_owned();
    let req = SendRequest { project: &project_text, provider: ProviderId::Claude, model: None, effort: None, text: "test", attach: &[], fresh: false, clean: None, yolo: false };
    let spawner = move |_: &o_core::providers::LaunchSpec, _: &str| {
        let mut command = fake_worker_launch()?;
        command.args(["--exact", "process_fixture", "--nocapture"]).env("O_RESOURCE_LOCK", project.join("running"));
        Ok(command)
    };
    assert!(lead::send(&home, &req, &spawner).is_ok());
    assert!(lead::send(&home, &req, &spawner).is_ok());
}

#[cfg(target_os = "linux")]
#[test]
fn linux_worker_completes_or_stops_but_keeps_unverified_reservations() {
    use o_core::run::events::CardStatus;
    use o_core::run::plan::Plan;
    use o_core::run::runner::{run_plan, stop_run, Spawner};
    use o_core::run::store::{active_markers, create_run};

    for (id, stop) in [("normal", false), ("stopped", true)] {
        let (home, project) = fixture();
        let card = card(id, vec![res(ResourceKind::External, "shared", ResourceMode::Write)]);
        let plan = Plan { title: "test".into(), project: project.to_string_lossy().into_owned(), cards: vec![card.clone()] };
        create_run(&home, id, &plan).unwrap();
        let worker_home = home.clone();
        let descendant = project.join("child.pid");
        let child_pid_file = descendant.clone();
        let worker = std::thread::spawn(move || {
            let spawner: Arc<Spawner> = Arc::new(move |_, _| {
                let mut command = fake_worker_launch()?;
                command.args(["--exact", "process_fixture", "--nocapture"]);
                if stop { command.env("O_RESOURCE_FAKE", "1"); }
                else { command.env("O_RESOURCE_DESCENDANT", &child_pid_file); }
                Ok(command)
            });
            run_plan(&worker_home, id, &plan, None, spawner).unwrap()
        });
        if stop {
            for _ in 0..500 {
                if !active_markers(&home).is_empty() { break; }
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(!active_markers(&home).is_empty());
            stop_run(&home, id).unwrap();
        }
        assert_eq!(worker.join().unwrap(), if stop { CardStatus::Stopped } else { CardStatus::Done });
        assert!(resource_held(&home, id, id).unwrap());
        assert!(matches!(reserve(&home, &project, "next", &card), Reserved::Busy(_)));
        if !stop {
            let pid = std::fs::read_to_string(descendant).unwrap().parse().unwrap();
            assert!(o_core::sys::proc::is_alive(pid));
        }
    }
}

#[cfg(windows)]
#[test]
fn simultaneous_runners_serialize_a_shared_writer() {
    use o_core::run::events::CardStatus;
    use o_core::run::plan::Plan;
    use o_core::run::runner::{run_plan, Spawner};
    use o_core::run::store::create_run;
    let (home, project) = fixture();
    let lock = project.join("running");
    let barrier = Arc::new(Barrier::new(3));
    let mut threads = Vec::new();
    for id in ["one", "two"] {
        let plan = Plan { title: "test".into(), project: project.to_string_lossy().into_owned(), cards: vec![card(id, vec![res(ResourceKind::External, "shared", ResourceMode::Write)])] };
        create_run(&home, id, &plan).unwrap();
        let (home, lock, barrier) = (home.clone(), lock.clone(), Arc::clone(&barrier));
        threads.push(std::thread::spawn(move || {
            let spawner: Arc<Spawner> = Arc::new(move |_, _| {
                let mut command = fake_worker_launch()?;
                command.args(["--exact", "process_fixture", "--nocapture"]).env("O_RESOURCE_LOCK", &lock);
                Ok(command)
            });
            barrier.wait();
            run_plan(&home, id, &plan, None, spawner).unwrap()
        }));
    }
    barrier.wait();
    for thread in threads { assert_eq!(thread.join().unwrap(), CardStatus::Done); }
    assert!(!lock.with_extension("conflict").exists());
}

#[cfg(windows)]
#[test]
fn stopping_a_worker_releases_after_the_job_exits() {
    use o_core::run::events::CardStatus;
    use o_core::run::plan::Plan;
    use o_core::run::runner::{run_plan, stop_run, Spawner};
    use o_core::run::store::{active_markers, create_run};
    let (home, project) = fixture();
    let card = card("one", vec![res(ResourceKind::External, "shared", ResourceMode::Write)]);
    let plan = Plan { title: "test".into(), project: project.to_string_lossy().into_owned(), cards: vec![card.clone()] };
    create_run(&home, "one", &plan).unwrap();
    let worker_home = home.clone();
    let worker = std::thread::spawn(move || {
        let spawner: Arc<Spawner> = Arc::new(|_, _| {
            let mut command = fake_worker_launch()?;
            command.args(["--exact", "process_fixture"]).env("O_RESOURCE_FAKE", "1");
            Ok(command)
        });
        run_plan(&worker_home, "one", &plan, None, spawner).unwrap()
    });
    for _ in 0..500 {
        if !active_markers(&home).is_empty() { break; }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!active_markers(&home).is_empty());
    stop_run(&home, "one").unwrap();
    assert_eq!(worker.join().unwrap(), CardStatus::Stopped);
    assert!(!resource_held(&home, "one", "one").unwrap());
    assert!(matches!(reserve(&home, &project, "two", &card), Reserved::Granted(_)));
}

#[cfg(windows)]
#[test]
fn stop_never_kills_an_unowned_or_mismatched_marker_pid() {
    use o_core::run::plan::Plan;
    use o_core::run::runner::stop_run;
    use o_core::run::store::{create_run, mark_active, stop_requested, ActiveMarker};
    let (home, project) = fixture();
    let card = card("one", vec![]);
    let plan = Plan { title: "test".into(), project: project.to_string_lossy().into_owned(), cards: vec![card.clone()] };
    let mut children: Vec<_> = (0..2).map(|_| {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "process_fixture"]).env("O_RESOURCE_FAKE", "1").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        o_core::sys::proc::hide_window(&mut command).spawn().unwrap()
    }).collect();
    for run in ["legacy", "mismatch"] { create_run(&home, run, &plan).unwrap(); }
    let lease = granted(reserve(&home, &project, "mismatch", &card));
    link_slot(&home, &lease, children[1].id()).unwrap();
    for run in ["legacy", "mismatch"] {
        mark_active(&home, &ActiveMarker { run_id: run.into(), card_id: card.id.clone(), provider: card.provider, pid: children[0].id() }).unwrap();
        assert!(stop_run(&home, run).unwrap());
        assert!(stop_requested(&home, run));
    }
    std::thread::sleep(Duration::from_millis(300));
    for child in &mut children {
        assert!(child.try_wait().unwrap().is_none());
        child.kill().unwrap();
        child.wait().unwrap();
    }
    assert!(resource_held(&home, "mismatch", "one").unwrap());
}

#[cfg(windows)]
#[test]
fn stop_requested_during_start_is_handled_by_the_owner() {
    use o_core::run::events::CardStatus;
    use o_core::run::plan::Plan;
    use o_core::run::runner::{run_plan, stop_run, Spawner};
    use o_core::run::store::create_run;
    let (home, project) = fixture();
    let plan = Plan { title: "test".into(), project: project.to_string_lossy().into_owned(), cards: vec![card("one", vec![])] };
    create_run(&home, "one", &plan).unwrap();
    let stopping = home.clone();
    let spawner: Arc<Spawner> = Arc::new(move |_, _| {
        assert!(stop_run(&stopping, "one").unwrap());
        let mut command = fake_worker_launch()?;
        command.args(["--exact", "process_fixture"]).env("O_RESOURCE_FAKE", "1");
        Ok(command)
    });
    let started = std::time::Instant::now();
    assert_eq!(run_plan(&home, "one", &plan, None, spawner).unwrap(), CardStatus::Stopped);
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(!resource_held(&home, "one", "one").unwrap());
}

#[cfg(windows)]
#[test]
fn worker_descendant_exits_before_resources_are_released() {
    use o_core::run::events::CardStatus;
    use o_core::run::plan::Plan;
    use o_core::run::runner::{run_plan, Spawner};
    use o_core::run::store::create_run;
    let (home, project) = fixture();
    let pid_file = project.join("child.pid");
    let card = card("one", vec![res(ResourceKind::External, "shared", ResourceMode::Write)]);
    let plan = Plan { title: "test".into(), project: project.to_string_lossy().into_owned(), cards: vec![card.clone()] };
    create_run(&home, "one", &plan).unwrap();
    let child_pid_file = pid_file.clone();
    let spawner: Arc<Spawner> = Arc::new(move |_, _| {
        let mut command = fake_worker_launch()?;
        command.args(["--exact", "process_fixture", "--nocapture"]).env("O_RESOURCE_DESCENDANT", &child_pid_file);
        Ok(command)
    });
    assert_eq!(run_plan(&home, "one", &plan, None, spawner).unwrap(), CardStatus::Done);
    let pid = std::fs::read_to_string(pid_file).unwrap().parse().unwrap();
    assert!(!o_core::sys::proc::is_alive(pid));
    assert!(!resource_held(&home, "one", "one").unwrap());
    assert!(matches!(reserve(&home, &project, "two", &card), Reserved::Granted(_)));
}

#[cfg(windows)]
#[test]
fn failed_worker_releases_only_after_exit() {
    use o_core::run::events::CardStatus;
    use o_core::run::plan::Plan;
    use o_core::run::runner::{run_plan, Spawner};
    use o_core::run::store::create_run;
    let (home, project) = fixture();
    let card = card("one", vec![res(ResourceKind::External, "shared", ResourceMode::Write)]);
    let plan = Plan { title: "test".into(), project: project.to_string_lossy().into_owned(), cards: vec![card.clone()] };
    create_run(&home, "one", &plan).unwrap();
    let spawner: Arc<Spawner> = Arc::new(|_, _| {
        let mut command = fake_worker_launch()?;
        command.args(["--exact", "process_fixture"]);
        Ok(command)
    });
    assert_eq!(run_plan(&home, "one", &plan, None, spawner).unwrap(), CardStatus::Failed);
    assert!(!resource_held(&home, "one", "one").unwrap());
    assert!(matches!(reserve(&home, &project, "two", &card), Reserved::Granted(_)));
}
