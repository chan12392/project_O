use std::path::Path;
use std::process::{Command, Output};

fn o(base: &Path, args: &[&str], role: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_o")).args(args).env("LOCALAPPDATA", base).env("O_AGENT_ROLE", role).env("O_CARD_ID", "").output().unwrap()
}

#[test]
fn consent_and_agent_boundary() {
    let base = std::env::temp_dir().join(format!("o-agy-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let read = o(&base, &["agy", "--json"], "card");
    let value: serde_json::Value = serde_json::from_slice(&read.stdout).unwrap();
    assert_eq!(value["workers"], false);
    assert_eq!(value["confirmPaste"], true);
    assert_eq!(value["notice"]["quotes"].as_array().unwrap().len(), 3);
    assert!(!o(&base, &["agy", "--workers", "on"], "").status.success());
    assert!(!o(&base, &["agy", "--workers", "on", "--ack"], "card").status.success());
    let enabled = o(&base, &["agy", "--workers", "on", "--ack", "--json"], "");
    assert!(enabled.status.success(), "{}", String::from_utf8_lossy(&enabled.stderr));
    let value: serde_json::Value = serde_json::from_slice(&enabled.stdout).unwrap();
    assert_eq!(value["workers"], true);
    assert!(value["acknowledgedAt"].is_string());
}

#[test]
fn plan_check_json_reports_devin_profile_notice() {
    let base = std::env::temp_dir().join(format!("o-agy-plan-{}", std::process::id()));
    let project = base.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let plan = base.join("plan.json");
    std::fs::write(&plan, serde_json::to_vec(&serde_json::json!({"title":"test", "project":project.to_string_lossy(), "cards":[{"id":"a", "provider":"devin", "task":"문서 수정"}]})).unwrap()).unwrap();
    let out = o(&base, &["check", "--json", plan.to_str().unwrap()], "");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["ok"], true);
    assert!(value["notices"][0].as_str().unwrap().contains("Devin은 평소 프로필"));
}
