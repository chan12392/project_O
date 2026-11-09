use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use o_core::lead::{self, LeadEvent};
use o_core::run::events::{CardStatus, OEvent};
use o_core::run::store;
use serde_json::json;

fn setup(name: &str) -> (PathBuf, String) {
    let base = std::env::temp_dir().join(format!("o-lead-tab-stop-{name}-{}-{}", std::process::id(), Utc::now().timestamp_nanos_opt().unwrap()));
    let home = base.join("home");
    let project = base.join("project");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&project).unwrap();
    (home, project.to_string_lossy().into_owned())
}

fn live_run(home: &Path, project: &str) -> String {
    let id = store::new_run_id(Utc::now());
    let dir = store::run_dir(home, &id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("plan.json"), json!({ "title": "test", "project": project, "cards": [] }).to_string()).unwrap();
    store::mark_runner(home, &id).unwrap();
    assert!(store::run_is_live(home, &id));
    id
}

fn turn_lock(home: &Path, thread: &str) -> File {
    OpenOptions::new().read(true).write(true).create(true).truncate(false)
        .open(lead::thread_dir(home, thread).join("turn.lock")).unwrap()
}

#[test]
fn stop_all_stops_closed_tab_and_standalone_runs_only_in_its_project() {
    let (home, project) = setup("closed");
    let closed = lead::new_thread(&home, &project).unwrap();
    let open = lead::new_thread(&home, &project).unwrap();
    let dispatched = live_run(&home, &project);
    let event = LeadEvent::Dispatch {
        at: Utc::now().to_rfc3339(), plan_id: format!("{closed}-1"), run_id: dispatched.clone(),
        clean: false, profile: None, yolo: false,
    };
    let mut events = OpenOptions::new().append(true).open(lead::thread_dir(&home, &closed).join("events.jsonl")).unwrap();
    writeln!(events, "{}", serde_json::to_string(&event).unwrap()).unwrap();
    drop(events);
    lead::close(&home, &closed).unwrap();
    assert!(!lead::is_open(&home, &project, &closed));
    assert!(store::run_is_live(&home, &dispatched));

    let standalone = live_run(&home, &project);
    let other_project = Path::new(&project).with_file_name("other-project");
    fs::create_dir_all(&other_project).unwrap();
    let other = live_run(&home, &other_project.to_string_lossy());
    let finished = live_run(&home, &project);
    store::append_event(&home, &finished, &OEvent::RunEnd {
        at: Utc::now().to_rfc3339(), run_id: finished.clone(), status: CardStatus::Done,
    }).unwrap();

    let equivalent_project = format!("{}/", project.replace('\\', "/"));
    assert_eq!(lead::stop_all(&home, &equivalent_project).unwrap(), 2);
    assert!(store::stop_requested(&home, &dispatched));
    assert!(store::stop_requested(&home, &standalone));
    assert!(!store::stop_requested(&home, &other));
    assert!(store::run_is_live(&home, &other));
    assert!(!store::stop_requested(&home, &finished));
    assert!(lead::thread_dir(&home, &open).join("stop-all-at").exists());
    assert!(!lead::thread_dir(&home, &closed).join("stop-all-at").exists());
}

#[test]
fn stop_all_stops_standalone_runs_without_any_lead_tabs() {
    let (home, project) = setup("standalone");
    let run = live_run(&home, &project);
    assert!(lead::open_threads(&home, &project).is_empty());
    assert_eq!(lead::stop_all(&home, &project).unwrap(), 1);
    assert!(store::stop_requested(&home, &run));
}

#[test]
fn close_and_writer_retry_brief_turn_lock_contention() {
    for action in ["close", "writer-target", "writer-source"] {
        let (home, project) = setup(action);
        let first = lead::new_thread(&home, &project).unwrap();
        let second = lead::new_thread(&home, &project).unwrap();
        let target = if action == "writer-source" { &first } else { &second };
        let lock = turn_lock(&home, target);
        lock.lock().unwrap();
        let release = thread::spawn(move || {
            thread::sleep(Duration::from_millis(60));
            drop(lock);
        });
        let result = if action == "close" { lead::close(&home, &second) } else { lead::set_writer(&home, &second) };
        release.join().unwrap();
        assert!(result.is_ok(), "{action}: {result:?}");
        if action == "close" {
            assert!(!lead::is_open(&home, &project, &second));
        } else {
            let shown = lead::show(&home, Some(&project), Some(&second)).unwrap();
            assert_eq!(shown["threads"][0]["writer"], false);
            assert_eq!(shown["threads"][1]["writer"], true);
        }
    }
}

#[test]
fn close_and_writer_reject_running_markers_without_retrying() {
    for action in ["close", "writer-target", "writer-source"] {
        for locked in [false, true] {
            let (home, project) = setup(action);
            let first = lead::new_thread(&home, &project).unwrap();
            let second = lead::new_thread(&home, &project).unwrap();
            let target = if action == "writer-source" { &first } else { &second };
            let lock = turn_lock(&home, target);
            if locked { lock.lock().unwrap(); }
            let marker = lead::thread_dir(&home, target).join("running.json");
            fs::write(&marker, json!({ "provider": "claude", "owner": std::process::id(), "token": "running", "child": null }).to_string()).unwrap();
            let started = Instant::now();
            let result = if action == "close" { lead::close(&home, &second) } else { lead::set_writer(&home, &second) };
            assert!(started.elapsed() < Duration::from_millis(250), "{action} waited despite running.json");
            let error = result.unwrap_err();
            assert!(error.contains(if locked { "아직 답하는 중" } else { "종료를 확인" }), "{action}: {error}");
            assert!(marker.exists());
            assert!(lead::is_open(&home, &project, &second));
            drop(lock);
            fs::remove_file(marker).unwrap();
            let shown = lead::show(&home, Some(&project), Some(&first)).unwrap();
            assert_eq!(shown["threads"][0]["writer"], true);
            assert_eq!(shown["threads"][1]["writer"], false);
        }
    }
}
