//! Runner behaviour with a fake provider CLI. The fake is this test binary
//! itself: started with O_FAKE_CLI_LINES it prints those lines and exits, so
//! the tests need no node, shell or provider CLI.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{Duration as Span, Utc};
use o_core::events::{CardStatus, OEvent};
use o_core::js::{iso, parse_time};
use o_core::plan::{Card, Plan, ProviderId};
use o_core::policy::LIMITS;
use o_core::providers::LaunchSpec;
use o_core::runner::{run_plan, Spawner};
use o_core::store::{
    active_markers, clear_active, cooldown_times, create_run, mark_active, read_events, record_start, request_stop, set_cooldown, started_on,
    ActiveMarker,
};

const FAKE: &str = "O_FAKE_CLI_LINES";
/// `--dump-env <file>`: the fake writes its environment there as JSON and succeeds.
const DUMP_ENV: &str = "--dump-env";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some(DUMP_ENV) {
        let env: BTreeMap<String, String> = std::env::vars().collect();
        std::fs::write(&args[2], serde_json::to_string(&env).unwrap()).unwrap();
        println!(r#"{{"type":"result","subtype":"success","is_error":false,"result":"ok"}}"#);
        return;
    }
    if let Ok(lines) = std::env::var(FAKE) {
        let lines: Vec<String> = serde_json::from_str(&lines).expect("fake lines");
        for line in lines {
            println!("{line}");
        }
        return;
    }
    let tests: [(&str, fn()); 6] = [
        ("runs cards in parallel, records a limit as a provider cooldown", parallel_and_limit),
        ("a card whose provider is cooling down ends without launching", cooling_down),
        ("the day's cap is checked again when a card starts, not only at dispatch", daily_cap_at_start),
        ("a stop request ends the cards still waiting", stop_request),
        ("a card waits in the queue while its provider is full, then runs", queue_then_run),
        ("a clean run starts its cards with only the clean environment", clean_run),
    ];
    let mut failed = 0;
    for (name, test) in tests {
        let ok = std::panic::catch_unwind(test).is_ok();
        println!("test {name} ... {}", if ok { "ok" } else { "FAILED" });
        failed += usize::from(!ok);
    }
    println!("\nrunner: {} passed; {failed} failed", tests.len() - failed);
    if failed > 0 {
        std::process::exit(1);
    }
}

/// Stands in for a provider CLI: prints the given stream lines, then exits.
fn fake_cli(lines_for: impl Fn(&LaunchSpec) -> Vec<&'static str> + Send + Sync + 'static) -> Arc<Spawner> {
    Arc::new(move |spec: &LaunchSpec, cwd: &str| {
        let mut command = Command::new(std::env::current_exe()?);
        command.current_dir(cwd).env(FAKE, serde_json::to_string(&lines_for(spec)).unwrap());
        Ok(command)
    })
}

fn temp_home() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("o-test-{}-{}", std::process::id(), Utc::now().timestamp_nanos_opt().unwrap()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn card(id: &str, provider: ProviderId, task: &str) -> Card {
    Card { id: id.into(), provider, role: None, task: task.into(), model: None, effort: None }
}

fn plan(cards: Vec<Card>) -> Plan {
    Plan { title: "t".into(), project: std::env::temp_dir().to_string_lossy().into_owned(), cards }
}

fn ends_of(home: &std::path::Path, run_id: &str) -> BTreeMap<String, CardStatus> {
    read_events(home, run_id)
        .into_iter()
        .filter_map(|e| match e {
            OEvent::CardEnd { card_id, status, .. } => Some((card_id, status)),
            _ => None,
        })
        .collect()
}

fn quiet() -> Arc<Spawner> {
    fake_cli(|_| vec![r#"{"type":"result","subtype":"success","is_error":false,"result":"ok"}"#])
}

fn parallel_and_limit() {
    let home = temp_home();
    let mut p = plan(vec![card("plan", ProviderId::Claude, "설계해"), card("code", ProviderId::Codex, "구현해")]);
    p.title = "테스트".into();
    p.cards[0].role = Some("설계".into());
    p.cards[1].role = Some("코딩".into());
    create_run(&home, "r1", &p).unwrap();

    let status = run_plan(
        &home,
        "r1",
        &p,
        None,
        fake_cli(|spec| {
            if spec.bin == "claude" {
                vec![
                    r#"{"type":"system","subtype":"init","session_id":"s1"}"#,
                    r#"{"type":"assistant","message":{"content":[{"type":"text","text":"설계 끝"}]}}"#,
                    r#"{"type":"result","subtype":"success","is_error":false,"result":"설계 끝"}"#,
                ]
            } else {
                vec![r#"{"type":"thread.started","thread_id":"t1"}"#, r#"{"type":"turn.failed","error":{"message":"usage limit reached"}}"#]
            }
        }),
    )
    .unwrap();

    assert_eq!(status, CardStatus::Failed);
    let events = read_events(&home, "r1");
    assert_eq!(ends_of(&home, "r1"), BTreeMap::from([("plan".into(), CardStatus::Done), ("code".into(), CardStatus::Failed)]));
    assert_eq!(events.last().map(OEvent::kind), Some("run.end"));
    assert!(events.iter().any(|e| matches!(e, OEvent::CardLimit { card_id, .. } if card_id == "code")));
    let until = cooldown_times(&home).unwrap().get(&ProviderId::Codex).cloned().unwrap_or_default();
    assert!(parse_time(&until).is_some_and(|t| t > Utc::now()));
    assert_eq!(started_on(&home, Utc::now()).unwrap(), 2);
    assert!(active_markers(&home).is_empty());
    // The run start carries labels, and every card start a pid.
    assert!(events.iter().any(|e| matches!(e, OEvent::RunStart { cards, .. } if cards[0].label.as_deref() == Some("설계"))));
    assert!(events.iter().filter(|e| matches!(e, OEvent::CardStart { pid: Some(_), .. })).count() == 2);
}

fn cooling_down() {
    let home = temp_home();
    let p = plan(vec![card("a", ProviderId::Codex, "x")]);
    create_run(&home, "r2", &p).unwrap();
    set_cooldown(&home, ProviderId::Codex, &iso(Utc::now() + Span::seconds(60)), Utc::now()).unwrap();
    let launched = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&launched);
    let inner = quiet();
    let spawner: Arc<Spawner> = Arc::new(move |spec: &LaunchSpec, cwd: &str| {
        seen.store(true, Ordering::SeqCst);
        inner(spec, cwd)
    });
    run_plan(&home, "r2", &p, None, spawner).unwrap();
    assert!(!launched.load(Ordering::SeqCst));
    assert_eq!(started_on(&home, Utc::now()).unwrap(), 0);
    assert_eq!(ends_of(&home, "r2"), BTreeMap::from([("a".into(), CardStatus::Failed)]));
}

fn daily_cap_at_start() {
    let home = temp_home();
    for i in 0..LIMITS.max_cards_per_day {
        record_start(&home, "earlier", &format!("c{i}"), ProviderId::Codex, Utc::now()).unwrap();
    }
    let p = plan(vec![card("a", ProviderId::Claude, "x")]);
    create_run(&home, "r3", &p).unwrap();
    assert_eq!(run_plan(&home, "r3", &p, None, quiet()).unwrap(), CardStatus::Failed);
    assert_eq!(ends_of(&home, "r3"), BTreeMap::from([("a".into(), CardStatus::Failed)]));
    assert_eq!(started_on(&home, Utc::now()).unwrap(), LIMITS.max_cards_per_day);
}

fn stop_request() {
    let home = temp_home();
    let p = plan(vec![card("a", ProviderId::Codex, "x")]);
    create_run(&home, "r4", &p).unwrap();
    request_stop(&home, "r4").unwrap();
    assert_eq!(run_plan(&home, "r4", &p, None, quiet()).unwrap(), CardStatus::Stopped);
    assert_eq!(ends_of(&home, "r4"), BTreeMap::from([("a".into(), CardStatus::Stopped)]));
}

fn queue_then_run() {
    let home = temp_home();
    // Live markers from another run fill agy's slots (this test process stands in as their pid).
    let slots = LIMITS.max_active_per_provider(ProviderId::Agy);
    for i in 0..slots {
        let marker = ActiveMarker { run_id: "other".into(), card_id: format!("b{i}"), provider: ProviderId::Agy, pid: std::process::id() };
        mark_active(&home, &marker).unwrap();
    }
    let p = plan(vec![card("a", ProviderId::Agy, "x")]);
    create_run(&home, "r5", &p).unwrap();
    let freeing = home.clone();
    let freer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        for i in 0..slots {
            clear_active(&freeing, "other", &format!("b{i}"));
        }
    });
    run_plan(&home, "r5", &p, None, fake_cli(|_| vec![r#"{"event":"result","result":{"status":"SUCCESS","response":"ok"}}"#])).unwrap();
    freer.join().unwrap();
    let kinds: Vec<&str> = read_events(&home, "r5").iter().map(OEvent::kind).collect();
    let at = |k: &str| kinds.iter().position(|x| *x == k).unwrap();
    assert!(at("card.queued") < at("card.start"));
    assert_eq!(ends_of(&home, "r5"), BTreeMap::from([("a".into(), CardStatus::Done)]));
}

fn clean_run() {
    let home = temp_home();
    let root = home.join("agents");
    let dump = home.join("env.json");
    std::env::set_var("ORCA_TERMINAL_ID", "t1");
    std::env::set_var("BEAKHO_RELEASE", "pinned");
    let p = plan(vec![card("a", ProviderId::Claude, "x")]);
    create_run(&home, "r6", &p).unwrap();
    let target = dump.clone();
    let spawner: Arc<Spawner> = Arc::new(move |_: &LaunchSpec, cwd: &str| {
        let mut command = Command::new(std::env::current_exe()?);
        command.current_dir(cwd).arg(DUMP_ENV).arg(&target);
        Ok(command)
    });
    assert_eq!(run_plan(&home, "r6", &p, Some(&root), spawner).unwrap(), CardStatus::Done);
    let env: BTreeMap<String, String> = serde_json::from_str(&std::fs::read_to_string(&dump).unwrap()).unwrap();
    let get = |name: &str| env.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone());
    let profile = root.join("claude").join("home");
    assert_eq!(get("O_RUN_ID").as_deref(), Some("r6"));
    assert_eq!(get("O_CARD_ID").as_deref(), Some("a"));
    assert_eq!(get("USERPROFILE"), Some(profile.to_string_lossy().into_owned()));
    assert_eq!(get("CLAUDE_CONFIG_DIR"), Some(profile.join(".claude").to_string_lossy().into_owned()));
    assert!(get("PATH").is_some());
    assert!(get("ORCA_TERMINAL_ID").is_none() && get("BEAKHO_RELEASE").is_none());
    assert!(profile.join(".claude").join("settings.json").is_file());
}
