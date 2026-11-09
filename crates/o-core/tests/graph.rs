use o_core::graph::{query, wiring};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn project() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "o-graph-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(path.join(".grafly/.graph")).unwrap();
    path
}

#[test]
fn load_and_query_actual_index_shape() {
    let project = project();
    assert!(wiring::load(&project).unwrap().is_none());
    let path = project.join(".grafly/.graph/wiring.json");
    fs::write(&path, r##"{"meta":{"version":1},"nodes":[
      {"id":"src/a.rs#a","name":"a","kind":"function","path":"src/a.rs","span":"L1-L2","signature":null,"exported":true,"summary":null},
      {"id":"src/b.rs#b","name":"b","kind":"function","path":"src/b.rs"}],
      "edges":[{"source":"src/b.rs#b","target":"src/a.rs#a","relation":"calls","confidence":"extracted"}],"ambiguousCalls":[]}"##).unwrap();
    let graph = wiring::load(&project).unwrap().unwrap();
    let callers = query::callers(&graph, "a", query::Direction::In, Some(1)).unwrap();
    assert_eq!(callers.nodes[1].node.name, "b");
    assert_eq!(query::blast(&graph, &["src/a.rs".into()], 1).nodes.len(), 2);
    assert!(query::skeleton(&graph, "src/a.rs")[0].exported);
    assert_eq!(query::map(&graph).files, 2);
    fs::write(&path, b"invalid JSON").unwrap();
    assert!(wiring::load(&project).is_err());
    fs::remove_file(&path).unwrap();
    fs::remove_dir(project.join(".grafly/.graph")).unwrap();
    fs::remove_dir(project.join(".grafly")).unwrap();
    fs::remove_dir(project).unwrap();
}

#[test]
fn rejects_index_over_size_limit() {
    let project = project();
    let path = project.join(".grafly/.graph/wiring.json");
    fs::File::create(&path)
        .unwrap()
        .set_len(64 * 1024 * 1024 + 1)
        .unwrap();
    assert!(wiring::load(&project).unwrap_err().contains("64 MiB"));
    fs::remove_file(&path).unwrap();
    fs::remove_dir(project.join(".grafly/.graph")).unwrap();
    fs::remove_dir(project.join(".grafly")).unwrap();
    fs::remove_dir(project).unwrap();
}

#[test]
fn settings_defaults_reject_invalid_updates_and_preserve_broken_file() {
    use o_core::graph::settings::{self, Change, GraphSettings};
    let home = std::env::temp_dir().join(format!("o-graph-settings-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    assert_eq!(settings::read(&home), GraphSettings::default());
    assert!(settings::update(&home, Change { depth: Some(0), ..Default::default() }).is_err());
    assert!(settings::update(&home, Change { max_nodes: Some(99), ..Default::default() }).is_err());
    fs::write(home.join("graph/settings.json"), b"{broken").unwrap();
    assert_eq!(settings::read(&home), GraphSettings::default());
    assert!(settings::update(&home, Change { effects: Some(false), ..Default::default() }).is_err());
    assert_eq!(fs::read(home.join("graph/settings.json")).unwrap(), b"{broken");
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn view_marks_changes_and_keeps_glow_when_capped() {
    use o_core::graph::{settings, view};
    use serde_json::{json, Value};
    use std::process::Command;

    let project = project();
    let home = std::env::temp_dir().join(format!("o-graph-state-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir_all(&home).unwrap();
    let git = |args: &[&str]| {
        let mut command = Command::new("git");
        command.arg("-C").arg(&project).args(args);
        let output = o_core::sys::proc::hide_window(&mut command).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    };
    git(&["init", "-q"]);
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(project.join("src/changed.rs"), "before").unwrap();
    git(&["add", "src/changed.rs"]);
    git(&["-c", "user.name=Graph Test", "-c", "user.email=graph@example.invalid", "commit", "-qm", "fixture"]);
    fs::write(project.join("src/changed.rs"), "after").unwrap();

    let mut nodes = vec![
        json!({"id":"src/changed.rs#target","name":"target","kind":"function","path":"src/changed.rs"}),
        json!({"id":"src/caller.rs#call","name":"call","kind":"function","path":"src/caller.rs"}),
    ];
    for n in 0..110 {
        nodes.push(json!({"id":format!("other/file{n}.rs"),"name":format!("file{n}.rs"),"kind":"file","path":format!("other/file{n}.rs")}));
    }
    fs::write(project.join(".grafly/.graph/wiring.json"), serde_json::to_vec(&json!({
        "nodes": nodes,
        "edges": [{"source":"src/caller.rs#call","target":"src/changed.rs#target","relation":"calls","confidence":"extracted"}]
    })).unwrap()).unwrap();
    let path = project.to_string_lossy().into_owned();
    fs::write(home.join("projects.json"), serde_json::to_vec(&json!({"registered":[path]})).unwrap()).unwrap();
    settings::update(&home, settings::Change { max_nodes: Some(100), ..Default::default() }).unwrap();
    let deep_file = o_core::graph::deep::path(&home, &project);
    fs::create_dir_all(deep_file.parent().unwrap()).unwrap();
    fs::write(deep_file, serde_json::to_vec(&json!({
        "at":"2026-01-01T00:00:00Z", "provider":"codex", "model":"test",
        "usage":{"input":1,"output":2}, "areas":[{"id":"d:src","name":"Sources","summary":"Source area"}]
    })).unwrap()).unwrap();

    let result = serde_json::to_value(view(&home, Some(&path)).unwrap()).unwrap();
    assert_eq!(result["scope"], "project");
    assert_eq!(result["indexed"], true);
    assert_eq!(result["truncated"], true);
    let nodes = result["nodes"].as_array().unwrap();
    let find = |id: &str| nodes.iter().find(|n| n["id"] == id).unwrap();
    assert_eq!(find("f:src/changed.rs")["glow"], "active");
    assert_eq!(find("f:src/caller.rs")["glow"], "path");
    assert_eq!(find("d:src")["label"], "Sources");
    assert_eq!(find("d:src")["summary"], "Source area");
    assert_eq!(result["deep"]["provider"], "codex");
    assert!(result["links"].as_array().unwrap().iter().any(|l| l["source"] == "f:src/caller.rs" && l["target"] == "f:src/changed.rs" && l["glow"] == true));

    let overall: Value = serde_json::to_value(view(&home, None).unwrap()).unwrap();
    assert_eq!(overall["scope"], "overall");
    assert_eq!(overall["indexed"], true);
    assert!(overall["nodes"].as_array().unwrap().iter().any(|n| n["kind"] == "project" && n["glow"] == "active"));
    assert!(overall["nodes"].as_array().unwrap().iter().any(|n| n["kind"] == "area" && n["path"] == "src" && n["glow"] == "active"));
    assert!(overall["nodes"].as_array().unwrap().iter().any(|n| n["kind"] == "area" && n["path"] == "src" && n["label"] == "Sources"));
    fs::remove_dir_all(project).unwrap();
    fs::remove_dir_all(home).unwrap();
}
