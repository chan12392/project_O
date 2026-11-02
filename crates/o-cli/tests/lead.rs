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
    for line in ["o lead send --project", "o lead dispatch <planId>", "o lead stop"] {
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

    // Antigravity cannot lead; nothing is recorded.
    assert_eq!(o(&base, &["lead", "send", "--project", &project, "--provider", "agy", "안녕"], b"").status.code(), Some(1));

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
    assert_eq!(json_of(&o(&base, &["lead", "stop"], b""))["stopped"], false);
}
