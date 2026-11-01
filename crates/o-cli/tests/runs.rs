//! `o runs --json` through the run index, with the real `o` against a
//! throwaway O home (LOCALAPPDATA/project_O).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;

fn o(base: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_o")).args(args).env("LOCALAPPDATA", base).env("O_RUN_ID", "").env("O_CARD_ID", "").output().unwrap()
}

fn listed(base: &Path, args: &[&str]) -> Vec<(String, usize)> {
    let out = o(base, args);
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    value["runs"].as_array().unwrap().iter().map(|r| (r["runId"].as_str().unwrap().to_string(), r["events"].as_array().unwrap().len())).collect()
}

fn setup(name: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("o-runs-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let home = base.join("project_O");
    (base, home)
}

/// A run folder whose events.jsonl starts with run.start (unless `at` is empty) and has `extra` messages.
fn run(home: &Path, run_id: &str, at: &str, project: &str, extra: usize) {
    let dir = home.join("runs").join(run_id);
    fs::create_dir_all(&dir).unwrap();
    let mut text = String::new();
    if !at.is_empty() {
        let start = serde_json::json!({ "t": "run.start", "at": at, "runId": run_id, "title": "t", "project": project, "cards": [] });
        text += &format!("{start}\n");
    }
    for i in 0..extra {
        text += &format!("{}\n", serde_json::json!({ "t": "msg", "at": at, "from": "lead", "to": "all", "text": format!("m{i}") }));
    }
    fs::write(dir.join("events.jsonl"), text).unwrap();
}

#[test]
fn runs_are_listed_newest_first_with_a_limit_and_a_project_filter() {
    let (base, home) = setup("filter");
    run(&home, "a", "2026-11-01T09:00:00.000Z", "C:/work/one", 1);
    run(&home, "b", "2026-11-01T11:00:00.000Z", "C:/work/two", 2);
    run(&home, "c", "2026-11-01T10:00:00.000Z", "C:/work/one/", 0);
    run(&home, "d", "", "", 3);

    // Ordered by the run.start time, not the folder name; a run without a start is left out.
    assert_eq!(listed(&base, &["runs", "--json"]), [("b".into(), 3), ("c".into(), 1), ("a".into(), 2)]);
    assert_eq!(listed(&base, &["runs", "--json", "--limit", "2"]), [("b".into(), 3), ("c".into(), 1)]);
    assert_eq!(listed(&base, &["runs", "--json", "--project", "C:/work/one"]), [("c".into(), 1), ("a".into(), 2)]);
    if cfg!(windows) {
        assert_eq!(listed(&base, &["runs", "--json", "--project", r"c:\work\ONE\"]), [("c".into(), 1), ("a".into(), 2)]);
    }
    assert_eq!(listed(&base, &["runs", "--json", "--project", "C:/work/three"]), []);
    assert_eq!(o(&base, &["runs", "--json", "--limit", "x"]).status.code(), Some(1));
}

#[test]
fn processes_reading_at_once_ingest_every_line_once() {
    let (base, home) = setup("parallel");
    run(&home, "a", "2026-11-01T09:00:00.000Z", "C:/p", 300);
    run(&home, "b", "2026-11-01T10:00:00.000Z", "C:/p", 50);
    let children: Vec<_> = (0..4)
        .map(|_| {
            Command::new(env!("CARGO_BIN_EXE_o"))
                .args(["runs", "--json"])
                .env("LOCALAPPDATA", &base)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in children {
        let out = child.wait_with_output().unwrap();
        assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
        let value: Value = serde_json::from_slice(&out.stdout).unwrap();
        let counts: Vec<usize> = value["runs"].as_array().unwrap().iter().map(|r| r["events"].as_array().unwrap().len()).collect();
        assert_eq!(counts, [51, 301]);
    }
    let rows: i64 = o_core::index::open(&home).unwrap().query_row("SELECT count(*) FROM events", [], |r| r.get(0)).unwrap();
    assert_eq!(rows, 352);
}
