//! `o lead` with the real `o` against a throwaway O home. PATH is emptied so no
//! lead CLI can be found: nothing paid ever runs here.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;

fn o(base: &Path, args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_o"))
        .args(args)
        .env("LOCALAPPDATA", base)
        .env("PATH", "")
        .env_remove("O_AGENT_ROLE").env_remove("O_CARD_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

fn json_of(out: &Output) -> Value {
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

fn setup(name: &str) -> (PathBuf, String) {
    let base = std::env::temp_dir().join(format!("o-cli-lead-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let project = base.join("proj");
    fs::create_dir_all(&project).unwrap();
    (base, project.to_string_lossy().into_owned())
}

#[test]
fn help_lists_the_lead_commands() {
    let (base, _) = setup("help");
    let help = String::from_utf8_lossy(&o(&base, &["help"], b"").stdout).into_owned();
    for line in ["o lead send --project", "o lead dispatch <planId>", "o lead stop", "o lead models --provider", "o lead close <threadId>", "o lead writer <threadId>", "--thread T"] {
        assert!(help.contains(line), "{line}");
    }
}

#[test]
fn show_attach_send_and_stop_through_the_cli() {
    let (base, project) = setup("flow");
    let empty = json_of(&o(&base, &["lead", "show", "--json"], b""));
    assert!(empty["thread"].is_null() && empty["projects"] == serde_json::json!([]));

    // Attachments come in on stdin and land beside O's folder, not in it.
    let saved = json_of(&o(&base, &["lead", "attach", "--project", &project, "--name", "화면.png", "-"], b"png bytes"));
    let path = saved["path"].as_str().unwrap();
    assert!(Path::new(path).starts_with(base.join("project_O-lead")));
    assert_eq!(fs::read(path).unwrap(), b"png bytes");
    assert_eq!(o(&base, &["lead", "attach", "--project", &project, "--name", "run.exe", "-"], b"MZ").status.code(), Some(1));

    // An effort the CLI does not take, or a model id of the wrong shape: refused, nothing recorded.
    assert_eq!(o(&base, &["lead", "send", "--project", &project, "--provider", "agy", "--effort", "minimal", "안녕"], b"").status.code(), Some(1));
    assert_eq!(o(&base, &["lead", "send", "--project", &project, "--model", "opus 5", "안녕"], b"").status.code(), Some(1));

    // Without CLI metadata, aliases must not claim an unverified version.
    let models = json_of(&o(&base, &["lead", "models", "--provider", "claude"], b""));
    assert_eq!(models["models"][1]["label"], "opus (버전 미확인)");
    assert!(models["models"][1]["hint"].is_null());
    assert_eq!(models["models"][0]["label"], "fable (버전 미확인)");
    assert_eq!(models["stale"], true);
    assert_eq!(o(&base, &["lead", "models"], b"").status.code(), Some(1));

    // With no CLI on PATH the turn is still on record, ending in lead.error.
    let sent = json_of(&o(&base, &["lead", "send", "--project", &project, "--effort", "low", "--attach", path, "-"], "계획해 줘".as_bytes()));
    let kinds: Vec<&str> = sent["events"].as_array().unwrap().iter().map(|e| e["t"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["lead.user", "lead.error"]);
    assert_eq!(sent["events"][0]["text"], "계획해 줘");
    assert_eq!(sent["events"][0]["attachments"][0]["path"], path);

    let shown = json_of(&o(&base, &["lead", "show", "--project", &project, "--json"], b""));
    assert_eq!(shown["thread"]["threadId"], sent["threadId"]);
    assert_eq!(shown["thread"]["events"].as_array().unwrap().len(), 3);
    assert!(shown["thread"]["busy"].is_null());

    let fresh = json_of(&o(&base, &["lead", "new", "--project", &project], b""));
    assert_ne!(fresh["threadId"], sent["threadId"]);

    assert_eq!(o(&base, &["lead", "dispatch", "not-a-plan"], b"").status.code(), Some(1));
    // The profile flag is checked before anything else; the removed switch is an unknown argument.
    let bad = o(&base, &["lead", "dispatch", "not-a-plan", "--profile", "clean"], b"");
    assert!(String::from_utf8_lossy(&bad.stderr).contains("--profile은 project-o 또는 own"));
    let removed = o(&base, &["dispatch", "--efficiency", "on", "-"], b"{}");
    assert_eq!(removed.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&removed.stderr).contains("알 수 없는 인자: --efficiency"));
    assert_eq!(json_of(&o(&base, &["lead", "stop"], b""))["stopped"], false);
}

#[test]
fn explicit_thread_routes_show_send_attach_and_stop() {
    let (base, project) = setup("thread-routing");
    let first = json_of(&o(&base, &["lead", "new", "--project", &project], b""))["threadId"].as_str().unwrap().to_string();
    let second = json_of(&o(&base, &["lead", "new", "--project", &project], b""))["threadId"].as_str().unwrap().to_string();
    let sent = json_of(&o(&base, &["lead", "send", "--project", &project, "--thread", &first, "첫 탭 요청"], b""));
    assert_eq!(sent["threadId"], first);
    assert_eq!(sent["events"][0]["text"], "첫 탭 요청");

    let current = json_of(&o(&base, &["lead", "show", "--project", &project, "--json"], b""));
    assert_eq!(current["thread"]["threadId"], first);
    let other = json_of(&o(&base, &["lead", "show", "--thread", &second, "--json"], b""));
    assert_eq!(other["thread"]["threadId"], second);
    assert_eq!(other["thread"]["events"].as_array().unwrap().len(), 1);
    assert_eq!(other["threads"].as_array().unwrap().len(), 2);
    let current_path = base.join("project_O/leads/current.json");
    let current_before_attach = fs::read(&current_path).unwrap();
    assert_eq!(json_of(&o(&base, &["lead", "show", "--project", &project, "--json"], b""))["thread"]["threadId"], first);
    let saved_other = json_of(&o(&base, &["lead", "attach", "--project", &project, "--thread", &second, "--name", "other.txt", "-"], b"second tab"));
    assert!(Path::new(saved_other["path"].as_str().unwrap()).starts_with(base.join("project_O-lead").join(&second)));
    assert_eq!(fs::read(&current_path).unwrap(), current_before_attach, "attaching to a tab must not select it");

    let saved = json_of(&o(&base, &["lead", "attach", "--project", &project, "--thread", &first, "--name", "notes.txt", "-"], b"first tab"));
    let path = Path::new(saved["path"].as_str().unwrap());
    assert!(path.starts_with(base.join("project_O-lead").join(&first)));
    assert_eq!(fs::read(path).unwrap(), b"first tab");
    assert_eq!(json_of(&o(&base, &["lead", "stop", "--thread", &first], b""))["stopped"], false);

    let other_project = base.join("other");
    fs::create_dir_all(&other_project).unwrap();
    let other_project = other_project.to_str().unwrap();
    for args in [
        vec!["lead", "show", "--project", other_project, "--thread", &first, "--json"],
        vec!["lead", "send", "--project", other_project, "--thread", &first, "잘못된 프로젝트"],
        vec!["lead", "attach", "--project", other_project, "--thread", &first, "--name", "notes.txt", "-"],
        vec!["lead", "stop", "--project", other_project, "--thread", &first],
    ] {
        assert_eq!(o(&base, &args, b"").status.code(), Some(1), "{args:?}");
    }
    assert!(json_of(&o(&base, &["lead", "show", "--project", other_project, "--json"], b""))["thread"].is_null());
}

#[test]
fn tabs_enforce_limit_and_transfer_writer_without_deleting_history() {
    let (base, project) = setup("tab-lifecycle");
    let mut threads = Vec::new();
    for _ in 0..o_core::lead::MAX_OPEN {
        threads.push(json_of(&o(&base, &["lead", "new", "--project", &project], b""))["threadId"].as_str().unwrap().to_string());
    }
    assert_eq!(threads.len(), 4);
    assert_eq!(o(&base, &["lead", "new", "--project", &project], b"").status.code(), Some(1));
    let shown = json_of(&o(&base, &["lead", "show", "--project", &project, "--json"], b""));
    assert_eq!(shown["thread"]["threadId"], threads[3]);
    for (index, tab) in shown["threads"].as_array().unwrap().iter().enumerate() {
        assert_eq!(tab["threadId"], threads[index]);
        assert_eq!(tab["writer"], index == 0);
    }
    assert_eq!(shown["threads"].as_array().unwrap().len(), 4);

    json_of(&o(&base, &["lead", "writer", &threads[3]], b""));
    let written = json_of(&o(&base, &["lead", "show", "--project", &project, "--json"], b""));
    for (index, tab) in written["threads"].as_array().unwrap().iter().enumerate() {
        assert_eq!(tab["writer"], index == 3);
    }
    json_of(&o(&base, &["lead", "close", &threads[3]], b""));
    let closed = json_of(&o(&base, &["lead", "show", "--project", &project, "--json"], b""));
    assert_eq!(closed["thread"]["threadId"], threads[2]);
    assert_eq!(closed["threads"].as_array().unwrap().len(), 3);
    assert_eq!(closed["threads"][0]["writer"], true);
    assert!(!o_core::lead::read_thread(&base.join("project_O"), &threads[3]).is_empty());
    let fallback = json_of(&o(&base, &["lead", "show", "--thread", &threads[3], "--json"], b""));
    assert_eq!(fallback["thread"]["threadId"], threads[2]);
    assert_eq!(fallback["threads"], closed["threads"]);

    for args in [
        vec!["lead", "send", "--project", &project, "--thread", &threads[3], "닫힌 탭"],
        vec!["lead", "attach", "--project", &project, "--thread", &threads[3], "--name", "closed.txt", "-"],
        vec!["lead", "stop", "--thread", &threads[3]],
        vec!["lead", "writer", &threads[3]],
    ] {
        assert_eq!(o(&base, &args, b"").status.code(), Some(1), "{args:?}");
    }
    // Invalid sends must not consume the newly available tab before request validation.
    assert_eq!(o(&base, &["lead", "send", "--project", &project, "--new", "--model", "bad model", "안녕"], b"").status.code(), Some(1));
    let fresh = json_of(&o(&base, &["lead", "send", "--project", &project, "--new", "새 탭 요청"], b""));
    let current = json_of(&o(&base, &["lead", "show", "--project", &project, "--json"], b""));
    assert_eq!(current["threads"].as_array().unwrap().len(), 4);
    assert_eq!(current["thread"]["threadId"], fresh["threadId"]);
    assert!(threads.iter().all(|thread| fresh["threadId"] != *thread));
}

#[test]
fn agents_cannot_close_tabs_or_take_writer() {
    let (base, project) = setup("tab-agent-refusal");
    let first = json_of(&o(&base, &["lead", "new", "--project", &project], b""))["threadId"].as_str().unwrap().to_string();
    let second = json_of(&o(&base, &["lead", "new", "--project", &project], b""))["threadId"].as_str().unwrap().to_string();
    for (marker, value) in [("O_AGENT_ROLE", "lead"), ("O_AGENT_ROLE", "card"), ("O_CARD_ID", "card-1")] {
        for action in ["close", "writer"] {
            let output = Command::new(env!("CARGO_BIN_EXE_o"))
                .args(["lead", action, &second])
                .env("LOCALAPPDATA", &base).env("PATH", "")
                .env_remove("O_AGENT_ROLE").env_remove("O_CARD_ID").env(marker, value)
                .output().unwrap();
            assert_eq!(output.status.code(), Some(2), "{marker} {action}");
            assert!(String::from_utf8_lossy(&output.stderr).contains("에이전트 안에서는"));
        }
    }
    let shown = json_of(&o(&base, &["lead", "show", "--project", &project, "--json"], b""));
    assert_eq!(shown["threads"].as_array().unwrap().len(), 2);
    assert_eq!(shown["threads"][0]["threadId"], first);
    assert_eq!(shown["threads"][0]["writer"], true);
    for action in ["close", "writer"] {
        let output = Command::new(env!("CARGO_BIN_EXE_o"))
            .args(["lead", action]).env("LOCALAPPDATA", &base)
            .env("O_AGENT_ROLE", "").env("O_CARD_ID", "").output().unwrap();
        assert_eq!(output.status.code(), Some(1));
    }
}
