#![cfg(windows)]
mod support;

use std::io::Write;
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::Utc;
use o_core::run::events::{CardStatus, OEvent};
use o_core::run::plan::Plan;
use o_core::run::runner::{run_plan, Spawner};
use o_core::run::store::{append_event, create_run, read_events, run_dir};

#[test]
fn fake_provider() {
    if std::env::var_os("O_DEPENDENCY_FIXTURE").is_some() {
        if let Some(marker) = std::env::var_os("O_DEPENDENCY_MARKER") { std::fs::write(marker, "started").unwrap(); }
        if let (Some(home), Some(run)) = (std::env::var_os("O_DEPENDENCY_HOME"), std::env::var_os("O_DEPENDENCY_RUN")) {
            append_event(std::path::Path::new(&home), &run.to_string_lossy(), &OEvent::CardTool { at: "test".into(), card_id: "dependent".into(), name: "fixture".into(), summary: "event lock released".into() }).unwrap();
        }
        let result = if std::env::var_os("O_DEPENDENCY_LARGE").is_some() { "x".repeat(13_000) } else { "short-result".into() };
        println!("{}", serde_json::json!({"type":"result","subtype":"success","is_error":false,"result":result}));
    }
}

#[test]
fn short_predecessors_are_verified_before_starting_the_dependent() {
    for scenario in ["valid", "large", "corrupt", "queued", "restarted", "failed", "late-queued", "late-corrupt", "late-partial"] {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let base = std::env::temp_dir().join(format!("o-dependency-{}-{nonce}", std::process::id()));
        let project = base.join("project");
        let home = base.join("state");
        std::fs::create_dir_all(&project).unwrap();
        let card = |id: &str, depends_on: &[&str]| serde_json::json!({
            "id": id, "provider": "claude", "task": id, "depends_on": depends_on,
            "resources": [{"kind": "external", "key": id, "mode": "write"}]
        });
        let plan: Plan = serde_json::from_value(serde_json::json!({
            "title": "test", "project": project.to_string_lossy(),
            "cards": [card("first", &[]), card("gate", &[]), card("dependent", &["first", "gate"])]
        })).unwrap();
        create_run(&home, scenario, &plan).unwrap();
        let launched = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&launched);
        let gate_home = home.clone();
        let marker = base.join("dependent-started");
        let child_marker = marker.clone();
        let spawner: Arc<Spawner> = Arc::new(move |spec, cwd| {
            match spec.env["O_CARD_ID"].as_str() {
                "gate" => {
                    let deadline = Instant::now() + Duration::from_secs(10);
                    while !read_events(&gate_home, scenario).iter().any(|event| matches!(event, OEvent::CardEnd { card_id, status: CardStatus::Done, .. } if card_id == "first")) {
                        if Instant::now() >= deadline { return Err(std::io::Error::other("first card did not finish")); }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    match scenario {
                        "corrupt" => {
                            let dir = run_dir(&gate_home, scenario);
                            let lock = std::fs::OpenOptions::new().write(true).open(dir.join("events.lock"))?;
                            lock.lock()?;
                            std::fs::OpenOptions::new().append(true).open(dir.join("events.jsonl"))?.write_all(b"{broken}\n")?;
                        }
                        "queued" => append_event(&gate_home, scenario, &OEvent::CardQueued { at: "test".into(), card_id: "first".into(), reason: "restart".into() })?,
                        "restarted" => append_event(&gate_home, scenario, &OEvent::CardStart { at: "test".into(), card_id: "first".into(), pid: None })?,
                        "failed" => append_event(&gate_home, scenario, &OEvent::CardEnd { at: "test".into(), card_id: "first".into(), status: CardStatus::Failed, result: None, exit_code: None })?,
                        _ => (),
                    }
                }
                "dependent" => {
                    let prompt = spec.stdin.as_deref().unwrap();
                    if scenario == "large" { assert!(prompt.contains("o context --result first")); }
                    else { assert!(prompt.contains("short-result")); }
                    observed.store(true, Ordering::SeqCst);
                    if scenario == "late-queued" {
                        append_event(&gate_home, scenario, &OEvent::CardQueued { at: "test".into(), card_id: "first".into(), reason: "restart".into() })?;
                    } else if matches!(scenario, "late-corrupt" | "late-partial") {
                        let dir = run_dir(&gate_home, scenario);
                        let lock = std::fs::OpenOptions::new().write(true).open(dir.join("events.lock"))?;
                        lock.lock()?;
                        let data: &[u8] = if scenario == "late-partial" { b"{partial" } else { b"{broken}\n" };
                        std::fs::OpenOptions::new().append(true).open(dir.join("events.jsonl"))?.write_all(data)?;
                    }
                }
                _ => (),
            }
            let mut command = support::fake_worker_launch()?;
            command.current_dir(cwd).args(["--exact", "fake_provider", "--nocapture"]).env("O_DEPENDENCY_FIXTURE", "1");
            if spec.env["O_CARD_ID"] == "dependent" { command.env("O_DEPENDENCY_MARKER", &child_marker).env("O_DEPENDENCY_HOME", &gate_home).env("O_DEPENDENCY_RUN", scenario); }
            if scenario == "large" { command.env("O_DEPENDENCY_LARGE", "1"); }
            Ok(command)
        });
        let status = run_plan(&home, scenario, &plan, None, spawner).unwrap();
        let allowed = matches!(scenario, "valid" | "large");
        assert_eq!(marker.exists(), allowed, "{scenario}");
        assert_eq!(launched.load(Ordering::SeqCst), allowed || scenario.starts_with("late-"), "{scenario}");
        assert_eq!(status, if allowed { CardStatus::Done } else { CardStatus::Failed }, "{scenario}");
        if scenario.starts_with("late-") {
            assert_eq!(o_core::run::store::started_on(&home, Utc::now()).unwrap(), 2, "{scenario}");
            assert!(o_core::run::store::active_markers(&home).is_empty(), "{scenario}");
            if scenario == "late-partial" {
                assert!(std::fs::read_to_string(run_dir(&home, scenario).join("events.jsonl")).unwrap().contains("{partial\n"));
            }
        }
        if allowed {
            let events = read_events(&home, scenario);
            let start = events.iter().position(|event| matches!(event, OEvent::CardStart { card_id, .. } if card_id == "dependent")).unwrap();
            let tool = events.iter().position(|event| matches!(event, OEvent::CardTool { card_id, name, .. } if card_id == "dependent" && name == "fixture")).unwrap();
            assert!(start < tool, "{scenario}");
        }
    }
}
