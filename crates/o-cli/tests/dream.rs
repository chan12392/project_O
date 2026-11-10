use std::fs;
use std::io::Write;
use std::process::{Command, Output, Stdio};

use serde_json::Value;

fn o(base: &std::path::Path, args: &[&str], input: Option<&str>, card: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_o"))
        .args(args)
        .env("LOCALAPPDATA", base.join("local"))
        .env("HOME", base.join("user"))
        .env("USERPROFILE", base.join("user"))
        .env("O_AGENT_ROLE", "")
        .env("O_CARD_ID", card)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input.unwrap_or_default().as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

fn output(base: &std::path::Path, args: &[&str], input: Option<&str>) -> Value {
    let result = o(base, args, input, "");
    assert!(result.status.success(), "{args:?}: {}", String::from_utf8_lossy(&result.stderr));
    serde_json::from_slice(&result.stdout).unwrap()
}

#[test]
fn materials_go_in_are_read_wired_and_come_out() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("o-cli-dream-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&base).unwrap();

    assert_eq!(output(&base, &["dream", "list", "--json"], None)["docs"].as_array().unwrap().len(), 0);
    let first = output(&base, &["dream", "add", "-"], Some(r##"{"kind":"text","title":"배포 점검표","text":"# 점검\n\n본문"}"##));
    let second = output(&base, &["dream", "add", "-"], Some(r#"{"kind":"text","text":"메모\n\n[[배포 점검표]]를 봅니다."}"#));
    let (first, second) = (first["path"].as_str().unwrap(), second["path"].as_str().unwrap());
    assert!(first.starts_with("외부 자료/"), "{first}");

    let list = output(&base, &["dream", "list"], None);
    assert_eq!(list["docs"].as_array().unwrap().len(), 2);
    let page = output(&base, &["dream", "read", second, "--json"], None);
    assert_eq!(page["title"], "메모");
    assert_eq!(page["links"][0], first);
    let graph = output(&base, &["dream", "graph", "--json"], None);
    assert_eq!(graph["nodes"][0]["id"], "dream");
    assert!(graph["links"].as_array().unwrap().iter().any(|l| l["kind"] == "references" && l["target"] == format!("f:{first}")));

    // A card neither adds nor removes.
    assert_eq!(o(&base, &["dream", "add", "-"], Some(r#"{"kind":"text","text":"x"}"#), "card-1").status.code(), Some(1));
    assert_eq!(o(&base, &["dream", "remove", first], None, "card-1").status.code(), Some(1));
    for refused in [vec!["dream"], vec!["dream", "write"], vec!["dream", "read"], vec!["dream", "read", "외부 자료/없음.md"], vec!["dream", "list", "--extra"], vec!["dream", "remove", "작업 성공/x.md"], vec!["dream", "add"]] {
        assert_eq!(o(&base, &refused, None, "").status.code(), Some(1), "{refused:?}");
    }
    assert_eq!(o(&base, &["dream", "add", "-"], Some("not json"), "").status.code(), Some(1));

    // Dreaming's changes: none yet, and only a person takes one back.
    assert_eq!(output(&base, &["dream", "changes", "--json"], None).as_array().unwrap().len(), 0);
    assert_eq!(o(&base, &["dream", "revert", "0000000000000000"], None, "").status.code(), Some(1));
    assert_eq!(o(&base, &["dream", "revert", "0000000000000000"], None, "card-1").status.code(), Some(1));

    assert_eq!(output(&base, &["dream", "remove", first], None)["ok"], true);
    assert_eq!(output(&base, &["dream", "list"], None)["docs"].as_array().unwrap().len(), 1);
    fs::remove_dir_all(base).unwrap();
}
