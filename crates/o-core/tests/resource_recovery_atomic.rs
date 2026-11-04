use std::sync::{Arc, Barrier};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use o_core::run::plan::{Card, ProviderId};
use o_core::run::store::{mark_spawning, recover_resource, reserve_slot, resource_leases, Reserved};

#[test]
fn spawn_transition_and_recovery_cannot_release_a_live_owner() {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("o-resource-atomic-{}-{nonce}", std::process::id()));
    let project = base.join("project");
    let home = base.join("state");
    std::fs::create_dir_all(&project).unwrap();
    let card = Card { id: "a".into(), provider: ProviderId::Claude, role: None, task: "test".into(), model: None, effort: None, depends_on: vec![], resources: vec![] };
    let Reserved::Granted(reservation) = reserve_slot(&home, "first", &card, project.to_str().unwrap(), Utc::now()).unwrap() else { panic!("reservation not granted") };
    let id = resource_leases(&home).unwrap()[0].id.parse::<i64>().unwrap();
    let reservation = Arc::new(reservation);
    let barrier = Arc::new(Barrier::new(4));
    let starters: Vec<_> = (0..2).map(|_| {
        let (home, reservation, barrier) = (home.clone(), Arc::clone(&reservation), Arc::clone(&barrier));
        std::thread::spawn(move || { barrier.wait(); mark_spawning(&home, &reservation).is_ok() })
    }).collect();
    let recovery_home = home.clone();
    let recovery_barrier = Arc::clone(&barrier);
    let recovery = std::thread::spawn(move || { recovery_barrier.wait(); recover_resource(&recovery_home, id).is_ok() });
    barrier.wait();
    assert_eq!(starters.into_iter().map(|thread| thread.join().unwrap()).filter(|success| *success).count(), 1);
    assert!(!recovery.join().unwrap());
    let leases = resource_leases(&home).unwrap();
    assert_eq!(leases.len(), 1);
    assert_eq!(leases[0].spawn_intent, Some(true));
    assert_eq!(leases[0].state, "unknown");
    assert!(matches!(reserve_slot(&home, "second", &card, project.to_str().unwrap(), Utc::now()).unwrap(), Reserved::Busy(_)));
}
