#![cfg(windows)]

use std::{fs, io::{BufRead, BufReader, Write}, path::Path, process::Command};
use o_core::{lead::{self, LeadEvent, SendRequest}, run::{events::{CardInfo, CardStatus, OEvent}, plan::{Card, Plan, ProviderId}, store::{append_event, create_run}}};
use serde_json::{json, Value};

#[test]
fn fake_stream_cli() {
    let Some(index) = std::env::var_os("O_TEST_STREAM_INDEX") else {
        if std::env::var_os("O_TEST_STREAM_PLAIN").is_some() { println!("{}", json!({"type":"result","subtype":"success","is_error":false,"result":"확인 불가"})); }
        return;
    };
    let rows = fs::read_to_string(&index).unwrap();
    let row: Value = rows.lines().map(|line| serde_json::from_str::<Value>(line).unwrap()).find(|row| row["type"] == "card.end").unwrap();
    let path = row["file"].as_str().unwrap();
    assert!(fs::write(&index, b"changed").is_err());
    assert!(fs::write(path, b"changed").is_err());
    let first = row["pageStart"].as_u64().unwrap() as usize - 1;
    let count = row["pageCount"].as_u64().unwrap() as usize;
    let text: String = BufReader::new(fs::File::open(path).unwrap()).lines().skip(first).take(count).map(|line| serde_json::from_str::<String>(&line.unwrap()).unwrap()).collect();
    let event: Value = serde_json::from_str(&text).unwrap();
    assert!(event["result"].as_str().unwrap().ends_with("마지막 결과"));
    assert_eq!(row["sha256"], o_core::run::result::digest(&text));
    println!("{}", json!({"type":"result","subtype":"success","is_error":false,"result":"읽었습니다"}));
}

#[test]
fn unavailable_result_emits_warning_without_failing_the_reply() {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("o-lead-unavailable-{}-{stamp}", std::process::id()));
    let home = base.join("state");
    let project_dir = base.join("project");
    fs::create_dir_all(&project_dir).unwrap();
    let project = project_dir.to_str().unwrap();
    let thread = lead::thread_for(&home, project, false).unwrap();
    writeln!(fs::OpenOptions::new().append(true).open(lead::thread_dir(&home, &thread).join("events.jsonl")).unwrap(), "{}", json!({"t":"lead.dispatch","at":"now","planId":"p","runId":"missing","clean":false})).unwrap();
    let spawner = |spec: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<Command> {
        assert!(spec.stdin.as_deref().unwrap().contains("상태 불명"));
        let mut command = Command::new(std::env::current_exe()?);
        command.args(["--exact", "fake_stream_cli", "--nocapture"]).env("O_TEST_STREAM_PLAIN", "1");
        Ok(command)
    };
    let request = SendRequest { project, provider: ProviderId::Claude, model: None, effort: None, text: "결과 확인", attach: &[], fresh: false, clean: None, yolo: false };
    let events = lead::send(&home, &request, &spawner).unwrap();
    assert!(events.iter().any(|event| matches!(event, LeadEvent::Warning { .. })));
    assert!(events.iter().any(|event| matches!(event, LeadEvent::Reply { text, .. } if text == "확인 불가")));
}

#[test]
fn lead_can_read_the_end_of_a_long_result_on_repeated_turns() {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("o-lead-stream-{}-{stamp}", std::process::id()));
    let home = base.join("state");
    let project_dir = base.join("project");
    fs::create_dir_all(&project_dir).unwrap();
    let project = project_dir.to_str().unwrap();
    let thread = lead::thread_for(&home, project, false).unwrap();
    let plan = Plan { title: "test".into(), project: project.into(), cards: vec![Card { id: "card".into(), provider: ProviderId::Claude, role: None, task: "test".into(), model: None, effort: None, depends_on: vec![] }] };
    create_run(&home, "run", &plan).unwrap();
    append_event(&home, "run", &OEvent::RunStart { at: "now".into(), run_id: "run".into(), title: "test".into(), project: project.into(), cards: vec![CardInfo { id: "card".into(), provider: ProviderId::Claude, label: None, role: None, model: None, effort: None }] }).unwrap();
    append_event(&home, "run", &OEvent::CardEnd { at: "now".into(), card_id: "card".into(), status: CardStatus::Done, result: Some(format!("{}마지막 결과", "앞부분\n".repeat(5000))), exit_code: Some(0) }).unwrap();
    append_event(&home, "run", &OEvent::RunEnd { at: "now".into(), run_id: "run".into(), status: CardStatus::Done }).unwrap();
    writeln!(fs::OpenOptions::new().append(true).open(lead::thread_dir(&home, &thread).join("events.jsonl")).unwrap(), "{}", json!({"t":"lead.dispatch","at":"now","planId":"p","runId":"run","clean":false})).unwrap();
    let spawner = |spec: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<Command> {
        let prompt = spec.stdin.as_deref().unwrap();
        assert!(prompt.contains("결과 미리보기") && prompt.contains("json-string-lines"));
        let index = spec.args.iter().position(|arg| arg == "--add-dir").unwrap();
        let index = fs::read_dir(Path::new(&spec.args[index + 1]))?.map(|entry| entry.unwrap().path()).find(|path| fs::read_to_string(path).is_ok_and(|text| text.contains("run-summary"))).unwrap();
        let mut command = Command::new(std::env::current_exe()?);
        command.args(["--exact", "fake_stream_cli", "--nocapture"]).env("O_TEST_STREAM_INDEX", index);
        Ok(command)
    };
    let request = SendRequest { project, provider: ProviderId::Claude, model: None, effort: None, text: "결과 확인", attach: &[], fresh: false, clean: None, yolo: false };
    for _ in 0..2 {
        let events = lead::send(&home, &request, &spawner).unwrap();
        assert!(events.iter().any(|event| matches!(event, LeadEvent::Reply { text, .. } if text == "읽었습니다")));
    }
}
