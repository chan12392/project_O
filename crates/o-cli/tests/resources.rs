use std::process::Command;

#[test]
fn check_reports_resource_declarations_and_rejects_unsafe_paths() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("o-cli-resource-{}-{nonce}", std::process::id()));
    let project = base.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let input = base.join("plan.json");
    let mut plan = serde_json::json!({"title":"test","project":project,"cards":[{"id":"a","provider":"claude","task":"test","resources":[{"kind":"project-path","key":"src","mode":"write"}]}]});
    let check = |plan: &serde_json::Value| {
        std::fs::write(&input, serde_json::to_vec(plan).unwrap()).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_o"));
        command.args(["check", input.to_str().unwrap()]).env("LOCALAPPDATA", &base);
        o_core::sys::proc::hide_window(&mut command).output().unwrap()
    };
    let output = check(&plan);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).contains("project-path"));
    plan["cards"][0]["resources"][0]["key"] = "../outside".into();
    assert_eq!(check(&plan).status.code(), Some(2));
}

#[test]
fn resources_shows_durable_identity_without_releasing_the_lease() {
    use o_core::run::plan::Card;
    use o_core::run::store::{reserve_slot, resource_held, Reserved};
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("o-cli-lease-{}-{nonce}", std::process::id()));
    let home = base.join("project_O");
    let project = base.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let card: Card = serde_json::from_value(serde_json::json!({"id":"a","provider":"claude","task":"test"})).unwrap();
    assert!(matches!(reserve_slot(&home, "r1", &card, project.to_str().unwrap(), chrono::Utc::now()).unwrap(), Reserved::Granted(_)));
    let mut command = Command::new(env!("CARGO_BIN_EXE_o"));
    command.args(["resources", "--json"]).env("LOCALAPPDATA", &base);
    let output = o_core::sys::proc::hide_window(&mut command).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let data: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(data["leases"][0]["runId"], "r1");
    assert_eq!(data["leases"][0]["ownerPid"], std::process::id());
    assert!(data["leases"][0]["ownerStarted"].is_string());
    assert_eq!(data["leases"][0]["state"], "unknown");
    assert!(resource_held(&home, "r1", "a").unwrap());
}

#[test]
fn person_can_release_an_unknown_lease_after_checking_children() {
    use o_core::run::store::{self, Reserved};
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("o-cli-confirm-{}-{nonce}", std::process::id()));
    let home = base.join("project_O");
    std::fs::create_dir_all(&base).unwrap();
    let card = serde_json::from_value(serde_json::json!({"id":"a","provider":"claude","task":"test"})).unwrap();
    let Reserved::Granted(lease) = store::reserve_slot(&home, "r1", &card, base.to_str().unwrap(), chrono::Utc::now()).unwrap() else { panic!("not granted") };
    store::mark_spawning(&home, &lease).unwrap();
    let exited = || {
        let mut command = if cfg!(windows) { let mut c = Command::new("cmd"); c.args(["/C", "exit 0"]); c } else { let mut c = Command::new("sh"); c.args(["-c", "exit 0"]); c };
        let mut child = o_core::sys::proc::hide_window(&mut command).spawn().unwrap();
        let pid = child.id();
        let started = o_core::sys::proc::started_at(pid).unwrap();
        child.wait().unwrap();
        (pid, started as i64)
    };
    let (owner, owner_started) = exited();
    let (worker, worker_started) = exited();
    let conn = rusqlite::Connection::open(home.join("o.db")).unwrap();
    conn.execute("UPDATE resource_leases SET owner_pid=?1,owner_started=?2,worker_pid=?3,worker_started=?4", rusqlite::params![owner, owner_started, worker, worker_started]).unwrap();
    let id = store::resource_leases(&home).unwrap()[0].id.clone();
    let mut command = Command::new(env!("CARGO_BIN_EXE_o"));
    command.args(["resources", "recover", &id, "--confirm-exited"]).env("LOCALAPPDATA", &base).env_remove("O_AGENT_ROLE").env_remove("O_CARD_ID");
    let output = o_core::sys::proc::hide_window(&mut command).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stderr).contains("자식 프로세스"));
    assert!(!store::resource_held(&home, "r1", "a").unwrap());
}

#[test]
fn resource_owner_fixture() {
    if let Some(base) = std::env::var_os("O_TEST_LEASE_OWNER") {
        let base = std::path::PathBuf::from(base);
        let card = serde_json::from_value(serde_json::json!({"id":"a","provider":"claude","task":"test"})).unwrap();
        assert!(matches!(o_core::run::store::reserve_slot(&base.join("project_O"), "r1", &card, base.to_str().unwrap(), chrono::Utc::now()).unwrap(), o_core::run::store::Reserved::Granted(_)));
        std::fs::write(base.join("ready"), "").unwrap();
        std::thread::sleep(std::time::Duration::from_secs(20));
    }
}

#[cfg(windows)]
#[test]
fn resources_recover_refuses_live_owner_then_recovers_unstarted_lease() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("o-cli-recover-{}-{nonce}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let mut owner = Command::new(std::env::current_exe().unwrap());
    owner.args(["--exact", "resource_owner_fixture"]).env("O_TEST_LEASE_OWNER", &base).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let mut owner = o_core::sys::proc::hide_window(&mut owner).spawn().unwrap();
    for _ in 0..500 { if base.join("ready").exists() { break; } std::thread::sleep(std::time::Duration::from_millis(10)); }
    assert!(base.join("ready").exists());
    let lease = o_core::run::store::resource_leases(&base.join("project_O")).unwrap().pop().unwrap();
    let recover = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_o"));
        command.args(["resources", "recover", &lease.id]).env("LOCALAPPDATA", &base);
        o_core::sys::proc::hide_window(&mut command).output().unwrap()
    };
    assert!(!recover().status.success());
    owner.kill().unwrap();
    owner.wait().unwrap();
    assert!(recover().status.success());
    assert!(!o_core::run::store::resource_held(&base.join("project_O"), "r1", "a").unwrap());
}
