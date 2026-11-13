use std::{fs, io::Write, path::PathBuf};

use o_core::{lead, run::store};
use serde_json::json;

fn setup(name: &str) -> (PathBuf, String) {
    let base = std::env::temp_dir().join(format!("o-lead-poll-{name}-{}", store::new_run_id(chrono::Utc::now())));
    let project = base.join("project");
    fs::create_dir_all(&project).unwrap();
    (base.join("home"), project.to_string_lossy().into_owned())
}

#[test]
fn polling_tabs_preserves_current_and_falls_back_after_close() {
    let (home, project) = setup("current");
    let first = lead::new_thread(&home, &project).unwrap();
    let second = lead::new_thread(&home, &project).unwrap();
    let path = home.join("leads/current.json");
    let before = fs::read(&path).unwrap();
    for id in [&first, &second, &first] {
        let shown = lead::show(&home, Some(&project), Some(id)).unwrap();
        assert_eq!(shown["thread"]["threadId"], *id);
        assert_eq!(fs::read(&path).unwrap(), before, "a polling window must not select its tab");
    }
    lead::close(&home, &first).unwrap();
    let after_close = fs::read(&path).unwrap();
    for requested in [&first, "missing-thread"] {
        let shown = lead::show(&home, Some(&project), Some(requested)).unwrap();
        assert_eq!(shown["thread"]["threadId"], second);
        assert_eq!(shown["threads"].as_array().unwrap().len(), 1);
        assert_eq!(fs::read(&path).unwrap(), after_close);
    }
    assert_eq!(lead::show(&home, None, Some(&first)).unwrap()["thread"]["threadId"], second);
    assert!(lead::show(&home, Some("another-project"), Some(&first)).is_err());
    assert!(lead::show(&home, Some(&project), Some("../outside")).is_err());
    lead::close(&home, &second).unwrap();
    let shown = lead::show(&home, Some(&project), Some(&first)).unwrap();
    assert!(shown["thread"].is_null());
    assert!(shown["threads"].as_array().unwrap().is_empty());
}

#[test]
fn tab_state_only_checks_the_latest_eight_dispatches_and_followups() {
    let (home, project) = setup("recent-runs");
    let id = lead::new_thread(&home, &project).unwrap();
    let mut events = fs::OpenOptions::new().append(true).open(lead::thread_dir(&home, &id).join("events.jsonl")).unwrap();
    for index in 0..9 {
        let run = format!("run-{index}");
        let dir = store::run_dir(&home, &run);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("runner.json"), json!({"pid": if index == 0 { std::process::id() } else { u32::MAX }}).to_string()).unwrap();
        let kind = if index % 2 == 0 { "lead.dispatch" } else { "lead.followup" };
        writeln!(events, "{}", json!({"t":kind,"at":"now","planId":"plan","runId":run,"clean":false,"sourceRunId":"source","cardId":"one"})).unwrap();
    }
    let shown = lead::show(&home, Some(&project), Some(&id)).unwrap();
    assert_eq!(shown["threads"][0]["state"], "idle", "the ninth-oldest live run is outside the display window");
    assert_eq!(shown["projects"][0]["state"], "idle");
    store::mark_runner(&home, "run-8").unwrap();
    let shown = lead::show(&home, Some(&project), None).unwrap();
    assert_eq!(shown["threads"][0]["state"], "working");
    store::append_event(&home, "run-8", &o_core::run::events::OEvent::RunEnd {
        at: "now".into(), run_id: "run-8".into(), status: o_core::run::events::CardStatus::Done,
    }).unwrap();
    assert_eq!(lead::show(&home, Some(&project), None).unwrap()["threads"][0]["state"], "idle");
}

#[test]
#[ignore = "micro-benchmark: run explicitly with --ignored --nocapture"]
fn finished_run_history_polling_benchmark() {
    let (home, project) = setup("finished-history");
    let id = lead::new_thread(&home, &project).unwrap();
    let mut events = fs::OpenOptions::new().append(true).open(lead::thread_dir(&home, &id).join("events.jsonl")).unwrap();
    for index in 0..8 {
        let run = format!("finished-{index}");
        let dir = store::run_dir(&home, &run);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("runner.json"), json!({"pid":u32::MAX}).to_string()).unwrap();
        writeln!(events, "{}", json!({"t":"lead.dispatch","at":"now","planId":"plan","runId":run,"clean":false})).unwrap();
    }
    for bytes in [0, 4 * 1024 * 1024] {
        for index in 0..8 {
            let run = format!("finished-{index}");
            let mut log = fs::File::create(store::run_dir(&home, &run).join("events.jsonl")).unwrap();
            // Real historical JSON, followed by the terminal event; total large logs = 32 MiB.
            writeln!(log, "{}", json!({"t":"historical.output","text":"x".repeat(bytes)})).unwrap();
            writeln!(log, "{}", json!({"t":"run.end","at":"now","runId":run,"status":"done"})).unwrap();
        }
        let start = std::time::Instant::now();
        for _ in 0..50 {
            assert_eq!(lead::show(&home, Some(&project), Some(&id)).unwrap()["threads"][0]["state"], "idle");
        }
        println!("finished log payload={} MiB, 50 polls={:?}", bytes * 8 / 1024 / 1024, start.elapsed());
    }
}
