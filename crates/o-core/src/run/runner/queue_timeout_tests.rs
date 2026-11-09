//! Tests for queued cards waiting on resource reservations.
use super::*;

#[test]
fn retained_reservation_fails_queued_card_without_starting_it() {
    let base = std::env::temp_dir().join(format!("o-queue-timeout-{}-{}", std::process::id(), Utc::now().timestamp_nanos_opt().unwrap()));
    let project = base.join("project");
    let home = base.join("state");
    fs::create_dir_all(&project).unwrap();
    let card = Card { id: "card".into(), provider: ProviderId::Claude, role: None, task: "test".into(), model: None, effort: None, depends_on: vec![], resources: vec![] };
    let project = project.to_string_lossy().into_owned();
    let Reserved::Granted(held) = reserve_slot(&home, "other", &card, &project, Utc::now()).unwrap() else { panic!("reservation missing") };
    let plan = Plan { title: "test".into(), project, cards: vec![card.clone()] };
    crate::run::store::create_run(&home, "other", &plan).unwrap();
    append_event(&home, "other", &OEvent::CardEnd { at: now_iso(), card_id: card.id, status: CardStatus::Failed, result: None, exit_code: None }).unwrap();
    crate::run::store::create_run(&home, "run", &plan).unwrap();
    let start = Instant::now();
    let spawner: Arc<Spawner> = Arc::new(|_, _| panic!("blocked card must not start"));
    assert_eq!(run_plan(&home, "run", &plan, None, spawner).unwrap(), CardStatus::Failed);
    assert!(start.elapsed() < Duration::from_secs(10));
    assert!(read_events(&home, "run").iter().any(|event| matches!(event, OEvent::CardEnd { result: Some(text), .. } if text.contains("종료된 카드의 예약"))));
    release_slot(&home, &held, false).unwrap();
}

#[test]
fn running_holder_waits_until_reservation_released() {
    let base = std::env::temp_dir().join(format!("o-queue-running-{}-{}", std::process::id(), Utc::now().timestamp_nanos_opt().unwrap()));
    let home = base.join("state");
    let project = base.join("project");
    fs::create_dir_all(&project).unwrap();
    let card = Card { id: "card".into(), provider: ProviderId::Claude, role: None, task: "test".into(), model: None, effort: None, depends_on: vec![], resources: vec![] };
    let project = project.to_string_lossy().into_owned();
    let held = match reserve_slot(&home, "other", &card, &project, Utc::now()).unwrap() { Reserved::Granted(held) => held, _ => panic!("reservation missing") };
    let plan = Plan { title: "test".into(), project, cards: vec![card] };
    crate::run::store::create_run(&home, "other", &plan).unwrap();
    append_event(&home, "other", &OEvent::CardStart { at: now_iso(), card_id: "card".into(), pid: Some(std::process::id()) }).unwrap();
    crate::run::store::create_run(&home, "run", &plan).unwrap();
    let attempted = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&attempted);
    let spawner: Arc<Spawner> = Arc::new(move |_, _| { seen.store(true, Ordering::Release); Err(io::Error::other("test spawn")) });
    let worker_home = home.clone();
    let worker = thread::spawn(move || run_plan(&worker_home, "run", &plan, None, spawner));
    thread::sleep(Duration::from_millis(2500));
    assert!(!worker.is_finished());
    assert!(!attempted.load(Ordering::Acquire));
    release_slot(&home, &held, false).unwrap();
    append_event(&home, "other", &OEvent::CardEnd { at: now_iso(), card_id: "card".into(), status: CardStatus::Done, result: None, exit_code: Some(0) }).unwrap();
    worker.join().unwrap().unwrap();
    assert!(attempted.load(Ordering::Acquire));
}

#[test]
fn best_effort_release_warns_without_claiming_tree_exit() {
    let base = std::env::temp_dir().join(format!("o-best-effort-{}-{}", std::process::id(), Utc::now().timestamp_nanos_opt().unwrap()));
    let home = base.join("state");
    fs::create_dir_all(&base).unwrap();
    let card = Card { id: "card".into(), provider: ProviderId::Claude, role: None, task: "test".into(), model: None, effort: None, depends_on: vec![], resources: vec![] };
    let project = base.to_string_lossy().into_owned();
    let plan = Plan { title: "test".into(), project: project.clone(), cards: vec![card.clone()] };
    crate::run::store::create_run(&home, "run", &plan).unwrap();
    let reservation = match reserve_slot(&home, "run", &card, &project, Utc::now()).unwrap() { Reserved::Granted(value) => value, _ => panic!("reservation missing") };
    crate::run::store::mark_spawning(&home, &reservation).unwrap();
    drop(Slot { home: &home, reservation, started: true, marked: false, exited: Arc::new(AtomicBool::new(true)), best_effort: true });
    assert!(!crate::run::store::resource_held(&home, "run", "card").unwrap());
    assert!(read_events(&home, "run").iter().any(|event| matches!(event, OEvent::CardTool { summary, .. } if summary.contains("자손 종료를 확인할 수 없어 예약을 해제"))));

    crate::run::store::create_run(&home, "uncertain", &plan).unwrap();
    let reservation = match reserve_slot(&home, "uncertain", &card, &project, Utc::now()).unwrap() { Reserved::Granted(value) => value, _ => panic!("reservation missing") };
    crate::run::store::mark_spawning(&home, &reservation).unwrap();
    drop(Slot { home: &home, reservation, started: true, marked: false, exited: Arc::new(AtomicBool::new(false)), best_effort: true });
    assert!(crate::run::store::resource_held(&home, "uncertain", "card").unwrap());
    assert!(!read_events(&home, "uncertain").iter().any(|event| matches!(event, OEvent::CardTool { .. })));
}
