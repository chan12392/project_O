use chrono::{Duration, TimeZone, Utc};
use o_core::run::yolo::{check_auto_run, AutoRunFacts, Limits, PastAutoRun};

#[test]
fn automatic_run_limits_reject_each_unsafe_case() {
    let now = Utc.with_ymd_and_hms(2026, 11, 3, 0, 0, 0).unwrap();
    let limits = Limits::default();
    let empty = [];
    let facts = AutoRunFacts { budget_on: true, cards: 3, running: &[], past: &empty, hash: "plan", now };
    assert!(check_auto_run(&facts, limits).is_ok());
    assert!(check_auto_run(&AutoRunFacts { budget_on: false, ..facts }, limits).is_err());
    assert!(check_auto_run(&AutoRunFacts { cards: 4, ..facts }, limits).is_err());
    let running = ["run".into()];
    assert!(check_auto_run(&AutoRunFacts { running: &running, ..facts }, limits).is_err());
    let duplicate = [PastAutoRun { at: now - Duration::minutes(29), hash: "plan".into() }];
    assert!(check_auto_run(&AutoRunFacts { past: &duplicate, ..facts }, limits).is_err());
    let two = [PastAutoRun { at: now - Duration::minutes(40), hash: "a".into() }, PastAutoRun { at: now - Duration::minutes(50), hash: "b".into() }];
    assert!(check_auto_run(&AutoRunFacts { past: &two, ..facts }, limits).is_err());
    let old = [PastAutoRun { at: now - Duration::minutes(61), hash: "plan".into() }];
    assert!(check_auto_run(&AutoRunFacts { past: &old, ..facts }, limits).is_ok());
}

#[test]
fn settings_updates_preserve_other_switches_and_bad_budget_data_blocks_autorun() {
    use o_core::{run::yolo, usage::budget};
    use std::sync::{Arc, Barrier};
    let home = std::env::temp_dir().join(format!("o-yolo-settings-{}-{}", std::process::id(), Utc::now().timestamp_nanos_opt().unwrap()));
    let start = Arc::new(Barrier::new(3));
    let first = { let home = home.clone(); let start = start.clone(); std::thread::spawn(move || { start.wait(); yolo::write_enabled(&home, true).unwrap(); }) };
    let second = { let home = home.clone(); let start = start.clone(); std::thread::spawn(move || { start.wait(); budget::write_enabled(&home, false).unwrap(); }) };
    start.wait(); first.join().unwrap(); second.join().unwrap();
    assert!(yolo::read_enabled(&home));
    assert!(!budget::read_enabled_for_autorun(&home));
    std::fs::write(home.join("settings.json"), "invalid").unwrap();
    assert!(!budget::read_enabled_for_autorun(&home));
    assert!(yolo::write_enabled(&home, false).is_err());
    assert_eq!(std::fs::read_to_string(home.join("settings.json")).unwrap(), "invalid");
}

#[test]
fn normalized_plan_hash_keeps_fields_and_ignores_text_spacing() {
    let plan: o_core::run::plan::Plan = serde_json::from_value(serde_json::json!({
        "title":"하나 둘", "project":"C:/work", "cards":[{"id":"one", "provider":"claude", "task":"첫째 둘째"}]
    })).unwrap();
    let mut same = plan.clone();
    same.cards[0].task = "첫째\n  둘째".into();
    assert_eq!(o_core::run::yolo::plan_hash(&plan), o_core::run::yolo::plan_hash(&same));
    same.cards[0].id = "other".into();
    assert_ne!(o_core::run::yolo::plan_hash(&plan), o_core::run::yolo::plan_hash(&same));
}
