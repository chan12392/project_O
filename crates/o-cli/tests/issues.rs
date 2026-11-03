use std::{fs, process::Command};

#[test]
fn records_and_lists_findings_through_cli() {
    let dir = std::env::temp_dir().join(format!("o-issues-cli-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap()));
    let home = if cfg!(windows) { dir.join("project_O") } else { dir.join(".local/state/project_O") };
    fs::create_dir_all(home.join("runs/run-1")).unwrap();
    fs::write(home.join("runs/run-1/plan.json"), r#"{"cards":[{"id":"card-1"}]}"#).unwrap();
    let file = dir.join("finding.json");
    fs::write(&file, serde_json::json!({"repo":"fixture/project", "runId":"run-1", "cardId":"card-1", "sha":"a".repeat(40), "summary":"finding", "reproduction":"steps", "evidence":["log:1"], "classification":"unclassified", "issue":null}).to_string()).unwrap();
    let command = || { let mut c = Command::new(env!("CARGO_BIN_EXE_o")); c.env(if cfg!(windows) { "LOCALAPPDATA" } else { "HOME" }, &dir).env_remove("O_AGENT_ROLE").env_remove("O_CARD_ID"); c };
    let saved = command().args(["issues", "record"]).arg(&file).output().unwrap();
    assert!(saved.status.success(), "{}", String::from_utf8_lossy(&saved.stderr));
    let listed = command().args(["issues", "list", "fixture/project"]).output().unwrap();
    assert!(listed.status.success());
    let values: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(values[0]["finding"]["runId"], "run-1");
    assert_eq!(values[0]["finding"]["evidence"][0], "log:1");
    fs::remove_dir_all(dir).unwrap();
}
