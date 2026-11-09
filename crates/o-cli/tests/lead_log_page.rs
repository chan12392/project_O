#![cfg(windows)]
//! Real CLI paging against a private, non-Git temporary folder.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};
use serde_json::{json, Value};

fn cli(base: &Path, project: &str, run: &str, hash: &str, offset: u64, agent: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_o"));
    command.args(["lead", "log-page", "--project", project, run, hash, &offset.to_string()])
        .env("LOCALAPPDATA", base).env("PATH", "").env_remove("O_CARD_ID").env_remove("O_AGENT_ROLE");
    if agent { command.env("O_CARD_ID", "card"); }
    o_core::sys::proc::hide_window(&mut command).output().unwrap()
}

fn ok(output: Output) -> Value {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn completed_logs_are_paged_verified_and_human_scoped() {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("o-cli-page-{}-{stamp}", std::process::id()));
    let home = base.join("project_O");
    let project_path = base.join("project");
    fs::create_dir_all(&project_path).unwrap();
    let project = project_path.to_str().unwrap();
    let thread = o_core::lead::thread_for(&home, project, false).unwrap();
    let thread_log = o_core::lead::thread_dir(&home, &thread).join("events.jsonl");
    let mut thread_log = fs::OpenOptions::new().append(true).open(thread_log).unwrap();
    for run in ["run", "linked", "huge"] {
        writeln!(thread_log, "{}", json!({"t":"lead.dispatch","at":"now","planId":"p","runId":run,"clean":false})).unwrap();
        fs::create_dir_all(home.join("runs").join(run)).unwrap();
    }
    drop(thread_log);
    let source = home.join("runs/run/events.jsonl");
    let value = format!("{}마지막", "a한글\\\"\n".repeat(7000));
    let original = format!("{}\n{}\n", json!({"t":"card.end","at":"now","cardId":"one","status":"done","result":value}), json!({"t":"run.end","at":"now","runId":"run","status":"done"}));
    fs::write(&source, &original).unwrap();
    let first = ok(cli(&base, project, "run", "-", 0, false));
    let hash = first["sha256"].as_str().unwrap().to_owned();
    let mut joined = first["text"].as_str().unwrap().to_owned();
    let mut next = first["nextOffset"].as_u64();
    while let Some(offset) = next {
        let page = ok(cli(&base, project, "run", &hash, offset, false));
        assert!(page["text"].as_str().unwrap().len() <= 4096);
        joined.push_str(page["text"].as_str().unwrap());
        next = page["nextOffset"].as_u64();
    }
    assert_eq!(joined, original);
    assert!(!cli(&base, project, "run", "-", 1, false).status.success());
    assert!(!cli(&base, project, "run", &hash, 1, true).status.success());
    assert!(!cli(&base, project, "other", "-", 0, false).status.success());
    let split = original.find('한').unwrap() as u64 + 1;
    assert!(!cli(&base, project, "run", &hash, split, false).status.success());
    let linked = home.join("runs/linked/events.jsonl");
    fs::hard_link(&source, &linked).unwrap();
    assert!(!cli(&base, project, "linked", "-", 0, false).status.success());
    assert!(!cli(&base, project, "run", "-", 0, false).status.success());

    let huge: PathBuf = home.join("runs/huge/events.jsonl");
    let big = format!("{}\n{}\n", json!({"t":"card.end","at":"now","cardId":"one","status":"done","result":"x".repeat(4 * 1024 * 1024 + 1)}), json!({"t":"run.end","at":"now","runId":"huge","status":"done"}));
    fs::write(&huge, &big).unwrap();
    let writer = fs::OpenOptions::new().write(true).open(&huge).unwrap();
    assert!(!cli(&base, project, "huge", "-", 0, false).status.success());
    drop(writer);
    let page = ok(cli(&base, project, "huge", "-", 0, false));
    assert_eq!(page["totalBytes"], big.len());
    let huge_hash = page["sha256"].as_str().unwrap();
    let last = ok(cli(&base, project, "huge", huge_hash, (big.len() - 100) as u64, false));
    assert_eq!(last["text"], &big[big.len() - 100..]);
    fs::write(&huge, big.replacen('x', "y", 1)).unwrap();
    assert!(!cli(&base, project, "huge", huge_hash, 4096, false).status.success());
    fs::write(&huge, big.trim_end_matches('\n')).unwrap();
    assert!(!cli(&base, project, "huge", "-", 0, false).status.success());
}
