use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::Utc;
use o_core::run::plan::{Card, ProviderId, Resource, ResourceKind, ResourceMode};
use o_core::run::store::{link_slot, release_slot, reserve_slot, resource_held, Reservation, Reserved};

fn fixture() -> (PathBuf, PathBuf) {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("o-resource-{}-{unique}", std::process::id()));
    let project = base.join("project");
    std::fs::create_dir_all(&project).unwrap();
    (base.join("state"), project)
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
fn process_fixture() {
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
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(resource_held(&home, "one", "one").unwrap());
    assert!(matches!(reserve(&home, &project, "two", &card("two", vec![res(ResourceKind::External, "shared", ResourceMode::Write)])), Reserved::Busy(_)));
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
    assert!(resource_held(&home, "one", "one").unwrap());
    assert!(matches!(reserve(&home, &project, "two", &first), Reserved::Busy(_)));
    release_slot(&home, &lease, true).unwrap();
    assert!(matches!(reserve(&home, &project, "two", &first), Reserved::Granted(_)));
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
                let mut command = std::process::Command::new(std::env::current_exe()?);
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
            let mut command = std::process::Command::new(std::env::current_exe()?);
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
        let mut command = std::process::Command::new(std::env::current_exe()?);
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
        let mut command = std::process::Command::new(std::env::current_exe()?);
        command.args(["--exact", "process_fixture"]);
        Ok(command)
    });
    assert_eq!(run_plan(&home, "one", &plan, None, spawner).unwrap(), CardStatus::Failed);
    assert!(!resource_held(&home, "one", "one").unwrap());
    assert!(matches!(reserve(&home, &project, "two", &card), Reserved::Granted(_)));
}
