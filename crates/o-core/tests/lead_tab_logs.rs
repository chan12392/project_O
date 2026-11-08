#![cfg(windows)]

use std::{fs, io::Write};
use o_core::{lead, run::store};
use serde_json::json;

#[test]
fn log_access_tracks_open_tabs_and_bounds_legacy_migration() {
    let base = std::env::temp_dir().join(format!("o-tab-logs-{}", store::new_run_id(chrono::Utc::now())));
    let home = base.join("home");
    let project_path = base.join("project");
    fs::create_dir_all(&project_path).unwrap();
    let project = project_path.to_str().unwrap();
    let first = lead::new_thread(&home, project).unwrap();
    let second = lead::new_thread(&home, project).unwrap();
    let mut events = fs::OpenOptions::new().append(true).open(lead::thread_dir(&home, &first).join("events.jsonl")).unwrap();
    for (run, kind) in [("run", "lead.dispatch"), ("followup", "lead.followup")] {
        writeln!(events, "{}", json!({"t":kind,"at":"now","planId":"p","runId":run,"clean":false,"sourceRunId":"run","cardId":"one"})).unwrap();
        fs::create_dir_all(store::run_dir(&home, run)).unwrap();
        fs::write(store::run_dir(&home, run).join("events.jsonl"), format!("{}\n", json!({"t":"run.end","at":"now","runId":run,"status":"done"}))).unwrap();
        assert_eq!(lead::run_thread(&home, project, run), Some(first.clone()));
        assert!(lead::log_page::read(&home, project, run, "-", 0).is_ok());
    }
    drop(events);
    assert_eq!(lead::current_thread(&home, project), Some(second.clone()));
    lead::close(&home, &first).unwrap();
    for run in ["run", "followup"] {
        assert_eq!(lead::run_thread(&home, project, run), Some(first.clone()), "closing retains the proven origin");
        assert!(lead::log_page::read(&home, project, run, "-", 0).is_err());
    }

    let mut events = fs::OpenOptions::new().append(true).open(lead::thread_dir(&home, &second).join("events.jsonl")).unwrap();
    writeln!(events, "{}", json!({"t":"lead.dispatch","at":"now","planId":"p","runId":"run","clean":false})).unwrap();
    drop(events);
    let open_path = home.join("leads/open.json");
    let current_path = home.join("leads/current.json");
    let current = fs::read(&current_path).unwrap();
    let key = o_core::context::normalize_project(project).to_lowercase();
    fs::write(&open_path, serde_json::to_vec(&json!({key: {"open": [], "writer": null}})).unwrap()).unwrap();
    assert!(lead::log_page::read(&home, project, "run", "-", 0).is_err(), "explicitly closed tabs never use current fallback");
    fs::remove_file(&open_path).unwrap();
    assert!(lead::log_page::read(&home, project, "run", "-", 0).is_ok(), "legacy current-only state stays readable");
    fs::write(&current_path, vec![b' '; 1024 * 1024 + 1]).unwrap();
    assert!(lead::log_page::read(&home, project, "run", "-", 0).is_err());
    fs::write(&current_path, current).unwrap();
    fs::write(&open_path, vec![b' '; 1024 * 1024 + 1]).unwrap();
    assert!(lead::log_page::read(&home, project, "run", "-", 0).is_err());
}

#[test]
fn standalone_dispatch_has_no_lead_origin_or_log_access() {
    let base = std::env::temp_dir().join(format!("o-tab-logs-{}", store::new_run_id(chrono::Utc::now())));
    let home = base.join("home");
    let project_path = base.join("project");
    fs::create_dir_all(&project_path).unwrap();
    let project = project_path.to_str().unwrap();
    let thread = lead::new_thread(&home, project).unwrap();
    fs::create_dir_all(store::run_dir(&home, "standalone")).unwrap();
    fs::write(store::run_dir(&home, "standalone").join("events.jsonl"), format!("{}\n", json!({"t":"run.end","at":"now","runId":"standalone","status":"done"}))).unwrap();
    assert_eq!(lead::current_thread(&home, project), Some(thread));
    assert_eq!(lead::run_thread(&home, project, "standalone"), None);
    assert!(lead::log_page::read(&home, project, "standalone", "-", 0).is_err());
}

#[test]
fn log_origin_requires_matching_project_and_bounded_complete_records() {
    let base = std::env::temp_dir().join(format!("o-tab-logs-{}", store::new_run_id(chrono::Utc::now())));
    let home = base.join("home");
    let project_path = base.join("project");
    let other_path = base.join("other");
    fs::create_dir_all(&project_path).unwrap();
    fs::create_dir_all(&other_path).unwrap();
    let project = project_path.to_str().unwrap();
    let thread = lead::new_thread(&home, other_path.to_str().unwrap()).unwrap();
    let path = lead::thread_dir(&home, &thread).join("events.jsonl");
    let mut events = fs::OpenOptions::new().append(true).open(&path).unwrap();
    writeln!(events, "{}", json!({"t":"lead.dispatch","at":"now","planId":"p","runId":"run","clean":false})).unwrap();
    drop(events);
    fs::create_dir_all(store::run_dir(&home, "run")).unwrap();
    fs::write(store::run_dir(&home, "run").join("events.jsonl"), format!("{}\n", json!({"t":"run.end","at":"now","runId":"run","status":"done"}))).unwrap();
    let key = o_core::context::normalize_project(project).to_lowercase();
    fs::write(home.join("leads/open.json"), serde_json::to_vec(&json!({key: {"open": [thread], "writer": null}})).unwrap()).unwrap();
    assert_eq!(lead::run_thread(&home, project, "run"), None);
    assert!(lead::log_page::read(&home, project, "run", "-", 0).is_err(), "an open-list entry cannot give another project ownership");

    let start = format!("{}\n", json!({"t":"lead.start","at":"now","threadId":thread,"project":project}));
    let dispatch = json!({"t":"lead.dispatch","at":"now","planId":"p","runId":"run","clean":false}).to_string();
    fs::write(&path, format!("{start}{dispatch}")).unwrap();
    assert!(lead::log_page::read(&home, project, "run", "-", 0).is_err(), "an incomplete lead record must not authorize log access");
    fs::write(&path, format!("{start}{}\n{dispatch}\n", " ".repeat(1024 * 1024 + 1))).unwrap();
    assert!(lead::log_page::read(&home, project, "run", "-", 0).is_err(), "origin lookup must retain the per-record bound");
}
