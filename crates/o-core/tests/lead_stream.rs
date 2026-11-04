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

#[test]
#[ignore = "실제 Codex 구독과 실행 한도를 사용하는 수동 통합 검사"]
fn official_codex_reads_the_last_part_of_a_result() {
    use o_core::{agents::clean, run::runner::official_command};
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let base = std::path::PathBuf::from(r"C:\tmp").join(format!("o-live-stream-{}-{stamp}", std::process::id()));
    let home = base.join("state");
    let project_dir = base.join("project");
    fs::create_dir_all(&project_dir).unwrap();
    let project = project_dir.to_str().unwrap();
    let thread = lead::thread_for(&home, project, false).unwrap();
    let plan = Plan { title: "test".into(), project: project.into(), cards: vec![Card { id: "card".into(), provider: ProviderId::Codex, role: None, task: "test".into(), model: None, effort: None, depends_on: vec![] }] };
    create_run(&home, "run", &plan).unwrap();
    append_event(&home, "run", &OEvent::RunStart { at: "now".into(), run_id: "run".into(), title: "test".into(), project: project.into(), cards: vec![CardInfo { id: "card".into(), provider: ProviderId::Codex, label: None, role: None, model: None, effort: None }] }).unwrap();
    let marker = format!("RESULT-{stamp}");
    append_event(&home, "run", &OEvent::CardEnd { at: "now".into(), card_id: "card".into(), status: CardStatus::Done, result: Some(format!("{}\n{marker}", "padding ".repeat(900))), exit_code: Some(0) }).unwrap();
    append_event(&home, "run", &OEvent::RunEnd { at: "now".into(), run_id: "run".into(), status: CardStatus::Done }).unwrap();
    writeln!(fs::OpenOptions::new().append(true).open(lead::thread_dir(&home, &thread).join("events.jsonl")).unwrap(), "{}", json!({"t":"lead.dispatch","at":"now","planId":"p","runId":"run","clean":false})).unwrap();
    let root = clean::default_root();
    let request = SendRequest { project, provider: ProviderId::Codex, model: None, effort: Some("low"), text: "run의 card 결과 원문을 끝까지 확인하고 마지막 줄의 RESULT-로 시작하는 식별자만 답하세요. 모르면 모른다고 답하세요.", attach: &[], fresh: false, clean: Some(&root), yolo: false };
    let events = lead::send(&home, &request, &official_command).unwrap();
    let answer = events.iter().find_map(|event| match event { LeadEvent::Reply { text, .. } => Some(text.as_str()), _ => None });
    assert!(answer.is_some_and(|text| text.contains(&marker)), "실제 응답이 결과 끝을 확인하지 못했습니다: {events:?}");
}

#[test]
fn old_codex_session_migrates_once_and_new_session_resumes() {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("o-codex-migrate-{}-{stamp}", std::process::id()));
    let home = base.join("state");
    let project = base.join("project");
    fs::create_dir_all(&project).unwrap();
    let thread = lead::thread_for(&home, project.to_str().unwrap(), false).unwrap();
    let mut events = fs::OpenOptions::new().append(true).open(lead::thread_dir(&home, &thread).join("events.jsonl")).unwrap();
    writeln!(events, "{}", json!({"t":"lead.session","at":"now","provider":"codex","sessionId":"old","clean":false})).unwrap();
    writeln!(events, "{}", json!({"t":"lead.dispatch","at":"now","planId":"p","runId":"missing","clean":false})).unwrap();
    let source = base.join("fake.rs");
    let binary = base.join("fake.exe");
    fs::write(&source, r####"
use std::io::{self, BufRead, Write};
fn main() {
    if std::env::args().any(|arg| arg == "--help") { println!("Usage: codex app-server"); return; }
    let mut log = std::fs::OpenOptions::new().create(true).append(true).open(std::env::var("TEST_LOG").unwrap()).unwrap();
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        writeln!(log, "{line}").unwrap();
        if line.contains("\"id\":1") { println!("{}", r#"{"id":1,"result":{}}"#); }
        if line.contains("\"id\":2") { println!("{}", r#"{"id":2,"result":{"thread":{"id":"new"}}}"#); }
        if line.contains("\"id\":3") {
            println!("{}", r#"{"id":3,"result":{"turn":{"id":"t"}}}"#);
            println!("{}", r#"{"method":"item/completed","params":{"threadId":"new","turnId":"t","item":{"type":"agentMessage","text":"확인"}}}"#);
            println!("{}", r#"{"method":"turn/completed","params":{"threadId":"new","turn":{"id":"t","status":"completed"}}}"#);
            io::stdout().flush().unwrap(); return;
        }
        io::stdout().flush().unwrap();
    }
}
"####).unwrap();
    let mut compiler = Command::new("rustc");
    compiler.arg(&source).arg("-o").arg(&binary);
    assert!(o_core::sys::proc::hide_window(&mut compiler).status().unwrap().success());
    let log = base.join("requests.jsonl");
    let child_log = log.clone();
    let spawner = move |spec: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<Command> {
        let mut command = Command::new(&binary);
        if spec.args.iter().any(|arg| arg == "--help") { command.arg("--help"); }
        command.env("TEST_LOG", &child_log);
        Ok(command)
    };
    let request = SendRequest { project: project.to_str().unwrap(), provider: ProviderId::Codex, model: None, effort: None, text: "결과 확인", attach: &[], fresh: false, clean: None, yolo: false };
    let first = lead::send(&home, &request, &spawner).unwrap();
    assert!(first.iter().any(|event| matches!(event, LeadEvent::Session { session_id, result_tools: Some(true), .. } if session_id == "new")));
    assert!(first.iter().any(|event| matches!(event, LeadEvent::Reply { text, .. } if text.contains("최근 일부만 전달"))));
    let second = lead::send(&home, &request, &spawner).unwrap();
    assert!(!second.iter().any(|event| matches!(event, LeadEvent::Warning { text, .. } if text.contains("세션을 새로"))));
    let calls: Vec<Value> = fs::read_to_string(log).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(calls.iter().filter(|v| v["method"] == "thread/start" && v["params"]["dynamicTools"].is_array()).count(), 1);
    assert_eq!(calls.iter().filter(|v| v["method"] == "thread/resume" && v["params"]["threadId"] == "new").count(), 1);
}
