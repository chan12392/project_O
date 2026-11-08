use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::{json, Value};

fn fixture(name: &str) -> (PathBuf, PathBuf) {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("o-cli-graph-{name}-{}-{nonce}", std::process::id()));
    let project = base.join("project");
    fs::create_dir_all(project.join(".grafly/.graph")).unwrap();
    fs::write(project.join(".grafly/.graph/wiring.json"), json!({
        "nodes": [
            {"id":"a#f", "name":"f", "kind":"function", "path":"a.rs"},
            {"id":"b#g", "name":"g", "kind":"function", "path":"b.rs"}
        ],
        "edges": [{"source":"b#g", "target":"a#f", "relation":"calls"}]
    }).to_string()).unwrap();
    (base, project)
}

fn o(base: &std::path::Path, args: &[&str], role: Option<&str>) -> Output {
    Command::new(env!("CARGO_BIN_EXE_o"))
        .args(args)
        .env("LOCALAPPDATA", base.join("local"))
        .env("HOME", base.join("user"))
        .env("USERPROFILE", base.join("user"))
        .env("O_AGENT_ROLE", role.unwrap_or(""))
        .env("O_CARD_ID", "")
        .output().unwrap()
}

fn output(base: &std::path::Path, args: &[&str]) -> Value {
    let result = o(base, args, None);
    assert!(result.status.success(), "{args:?}: {}", String::from_utf8_lossy(&result.stderr));
    serde_json::from_slice(&result.stdout).unwrap()
}

#[test]
fn settings_json_and_agent_refusals() {
    let (base, project) = fixture("settings");
    let project = project.to_str().unwrap();
    assert_eq!(output(&base, &["graph", "settings", "--json"])["effects"], true);
    assert_eq!(output(&base, &["graph", "settings", "--json"])["colorBy"], "language");
    assert_eq!(output(&base, &["graph", "settings", "--color-by", "area"])["colorBy"], "area");
    assert!(!o(&base, &["graph", "settings", "--color-by", "invalid"], None).status.success());
    assert_eq!(output(&base, &["graph", "settings", "--json"])["colorBy"], "area");
    let defaults = output(&base, &["graph", "settings", "--codex-model", "default", "--claude-model", "default", "--json"]);
    assert_eq!(defaults["codexModel"], "");
    assert_eq!(defaults["claudeModel"], "");
    assert_eq!(output(&base, &["graph", "settings", "--json"])["codexModel"], "");
    assert!(!o(&base, &["graph", "settings", "--codex-model", "bad model"], None).status.success());
    assert_eq!(output(&base, &["graph", "settings", "--effects", "off", "--depth", "3", "--json"])["depth"], 3);
    assert_eq!(output(&base, &["graph", "settings", "--json"])["effects"], false);
    for args in [vec!["graph", "build", "--project", project], vec!["graph", "deep", "--project", project], vec!["graph", "settings", "--effects", "on"]] {
        let result = o(&base, &args, Some("lead"));
        assert_eq!(result.status.code(), Some(2), "{args:?}");
        let card = Command::new(env!("CARGO_BIN_EXE_o")).args(&args).env("O_AGENT_ROLE", "").env("O_CARD_ID", "card-1").output().unwrap();
        assert_eq!(card.status.code(), Some(2), "card {args:?}");
    }
    assert!(o(&base, &["graph", "settings", "--json"], Some("lead")).status.success());
    assert!(o(&base, &["graph", "settings", "--depth", "5"], None).status.code() == Some(1));
    assert_eq!(output(&base, &["graph", "settings", "--json"])["depth"], 3);
}

#[test]
fn queries_use_the_requested_project_and_validate_options() {
    let (base, project) = fixture("queries");
    let project = project.to_str().unwrap();
    assert_eq!(output(&base, &["graph", "view", "--json"])["scope"], "overall");
    let view = output(&base, &["graph", "view", "--project", project, "--json"]);
    assert_eq!(view["scope"], "project");
    assert_eq!(view["indexed"], true);
    let part = output(&base, &["graph", "part", "--project", project, "--json"]);
    assert_eq!(part["scope"], "overall");
    assert_eq!(part["indexed"], true);
    assert!(part["nodes"].as_array().unwrap().iter().any(|n| n["kind"] == "project"));
    assert!(!part["nodes"].as_array().unwrap().iter().any(|n| n["id"] == "o:root"));
    assert!(!o(&base, &["graph", "part", "--json"], None).status.success());
    let callers = output(&base, &["graph", "callers", "f", "--project", project, "--direction", "in", "--depth", "all", "--json"]);
    assert_eq!(callers["nodes"].as_array().unwrap().len(), 2);
    assert!(o(&base, &["graph", "map", "--project", project, "--json"], Some("lead")).status.success());
    let skeleton = output(&base, &["graph", "skeleton", "a.rs", "--project", project, "--json"]);
    assert_eq!(skeleton[0]["name"], "f");
    assert_eq!(output(&base, &["graph", "map", "--project", project, "--json"])["files"], 2);
    assert!(o(&base, &["graph", "map", "--json"], None).status.code() == Some(1));
    assert!(o(&base, &["graph", "callers", "f", "--project", project, "--direction", "sideways"], None).status.code() == Some(1));
    assert!(o(&base, &["graph", "callers", "f", "--project", project, "--depth", "0"], None).status.code() == Some(1));
    assert!(o(&base, &["graph", "map", "--project", "--json"], None).status.code() == Some(1));
}

#[test]
fn blast_base_compares_with_the_requested_revision() {
    let (base, project) = fixture("blast");
    let git = |args: &[&str]| {
        let mut command = Command::new("git");
        command.current_dir(&project).args(args);
        let result = o_core::sys::proc::hide_window(&mut command).output().unwrap();
        assert!(result.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&result.stderr));
    };
    git(&["init", "-q"]);
    fs::write(project.join("a.rs"), "before").unwrap();
    git(&["add", "a.rs"]);
    git(&["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "initial"]);
    fs::write(project.join("a.rs"), "after").unwrap();
    git(&["add", "a.rs"]);
    git(&["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "change"]);
    let project = project.to_str().unwrap();
    let unchanged = output(&base, &["graph", "blast", "--project", project, "--json"]);
    assert!(unchanged["nodes"].as_array().unwrap().is_empty());
    let blast = output(&base, &["graph", "blast", "--project", project, "--base", "HEAD~1", "--json"]);
    assert_eq!(blast["nodes"].as_array().unwrap().len(), 2);
}
