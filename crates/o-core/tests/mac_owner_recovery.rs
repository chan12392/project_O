use std::path::PathBuf;

fn temporary_home() -> PathBuf {
    let base = std::env::temp_dir().join(format!("o-mac-recovery-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&base).unwrap();
    base
}

#[test]
fn migration_keeps_old_owner_scope_unknown() {
    let home = temporary_home();
    let db = rusqlite::Connection::open(home.join("o.db")).unwrap();
    db.execute_batch("CREATE TABLE resource_leases (slot_id INTEGER PRIMARY KEY, run_id TEXT NOT NULL, card_id TEXT NOT NULL, resources TEXT NOT NULL, owner_pid INTEGER, owner_started INTEGER, worker_pid INTEGER, worker_started INTEGER, spawn_intent INTEGER, exit_confirmed INTEGER, start_id INTEGER); INSERT INTO resource_leases (slot_id,run_id,card_id,resources) VALUES (1,'old','card','[]'); PRAGMA user_version = 6;").unwrap();
    drop(db);
    let db = o_core::run::index::open(&home).unwrap();
    let scope: Option<String> = db.query_row("SELECT owner_scope FROM resource_leases WHERE slot_id=1", [], |row| row.get(0)).unwrap();
    assert!(scope.is_none());
    assert_eq!(o_core::run::store::resource_leases(&home).unwrap()[0].state, "unknown");
    assert!(o_core::run::store::recover_resource(&home, 1).is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn mac_recovery_requires_same_boot_dead_owner_and_unstarted_record() {
    use o_core::run::plan::{Card, ProviderId};
    use o_core::run::store::{recover_resource, reserve_slot, resource_held, Reserved};
    use o_core::sys::proc::{process_scope, started_at};
    struct Owner(std::process::Child);
    impl Drop for Owner { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }
    let home = temporary_home();
    let project = home.join("project");
    std::fs::create_dir(&project).unwrap();
    let scope = process_scope().expect("boot session UUID must be available on the tested Mac");
    let mut owner = Owner(std::process::Command::new("/bin/sleep").arg("60").spawn().unwrap());
    let pid = owner.0.id();
    let started = i64::try_from(started_at(pid).expect("owner start time must be readable")).unwrap();
    assert!(!o_core::sys::proc::definitely_exited(pid, started as u64));
    assert!(o_core::sys::proc::definitely_exited(pid, (started + 1) as u64));
    assert!(!o_core::sys::proc::definitely_exited(0, 0));
    let card = Card { id: "card".into(), provider: ProviderId::Claude, role: None, task: "test".into(), model: None, effort: None, depends_on: vec![], resources: vec![] };
    let Reserved::Granted(_reservation) = reserve_slot(&home, "run", &card, project.to_str().unwrap(), chrono::Utc::now()).unwrap() else { panic!("reservation was not granted") };
    let slot: i64 = o_core::run::store::resource_leases(&home).unwrap()[0].id.parse().unwrap();
    let db = o_core::run::index::open(&home).unwrap();
    db.execute("UPDATE resource_leases SET owner_pid=?1,owner_started=?2,owner_scope=?3", rusqlite::params![pid, started, scope]).unwrap();
    assert!(recover_resource(&home, slot).is_err());
    owner.0.kill().unwrap();
    owner.0.wait().unwrap();
    db.execute("UPDATE resource_leases SET owner_scope=NULL", []).unwrap();
    assert!(recover_resource(&home, slot).is_err());
    db.execute("UPDATE resource_leases SET owner_scope='macos-boot:other'", []).unwrap();
    assert!(recover_resource(&home, slot).is_err());
    db.execute("UPDATE resource_leases SET owner_scope=?1,spawn_intent=NULL", [&scope]).unwrap();
    assert!(recover_resource(&home, slot).is_err());
    db.execute("UPDATE resource_leases SET owner_scope=?1,spawn_intent=1", [&scope]).unwrap();
    assert!(recover_resource(&home, slot).is_err());
    assert!(resource_held(&home, "run", "card").unwrap());
    db.execute("UPDATE resource_leases SET spawn_intent=0", []).unwrap();
    recover_resource(&home, slot).unwrap();
    assert!(!resource_held(&home, "run", "card").unwrap());
}
