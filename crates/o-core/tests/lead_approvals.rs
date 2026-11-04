//! A normal-mode Claude lead turn end to end with a fake Claude CLI (this test binary): it asks
//! O's approval server over MCP stdio, as Claude Code's `--permission-prompt-tool` does, and the
//! test answers through `lead::decide` as the app would. No model is called.

mod support;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use o_core::lead::{self, approvals, LeadEvent, SendRequest};
use o_core::providers::LaunchSpec;
use o_core::run::plan::ProviderId;
use serde_json::{json, Value};

/// The fake Claude: one Write that asks first, then the result it would print.
#[test]
fn fake_approving_cli() {
    let Ok(args) = std::env::var("O_TEST_APPROVE_ARGS") else { return };
    let args: Vec<String> = serde_json::from_str(&args).unwrap();
    let file = std::env::var("O_TEST_APPROVE_FILE").unwrap();
    let say = |line: Value| { println!("{line}"); std::io::stdout().flush().unwrap(); };
    say(json!({"type":"system","subtype":"init","session_id":"fake-session"}));
    let input = json!({"file_path": file, "content": "hi\nthere\n"});
    say(json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Write","input":input}]}}));
    // args: [_lead-approve, home, thread, token, project]
    let server = approvals::Server { home: PathBuf::from(&args[1]), thread_id: args[2].clone(), token: args[3].clone(), project: args[4].clone(), wait: approvals::WAIT };
    let (server_in, mut to_server) = std::io::pipe().unwrap();
    let (from_server, server_out) = std::io::pipe().unwrap();
    thread::spawn(move || approvals::serve(server, BufReader::new(server_in), server_out));
    writeln!(to_server, "{}", json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25"}})).unwrap();
    writeln!(to_server, "{}", json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"decide","arguments":{"tool_name":"Write","input":input,"tool_use_id":"toolu_1"}}})).unwrap();
    let mut lines = BufReader::new(from_server).lines();
    let reply = loop {
        let message: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
        if message["id"] == 2 { break message; }
    };
    let decision: Value = serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    if decision["behavior"] == "allow" {
        fs::write(&file, "hi\nthere\n").unwrap();
        say(json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"File created"}]},
            "tool_use_result":{"type":"create","filePath":file,"content":"hi\nthere\n","structuredPatch":[]}}));
    } else {
        say(json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","is_error":true,"content":decision["message"]}]},
            "tool_result_meta":[{"id":"toolu_1","permission_decision":{"decision":"reject"}}]}));
    }
    say(json!({"type":"result","subtype":"success","is_error":false,"result":format!("결과: {}", decision["behavior"].as_str().unwrap())}));
}

struct Case {
    home: PathBuf,
    project: String,
    file: PathBuf,
}

fn case(name: &str) -> Case {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("o-lead-approve-{name}-{}-{stamp}", std::process::id()));
    let project = base.join("project");
    fs::create_dir_all(&project).unwrap();
    Case { home: base.join("state"), file: project.join("hello.txt"), project: project.to_string_lossy().into_owned() }
}

/// Runs one turn; `answer` gets the first waiting request's id once it shows in `lead show`.
fn turn(case: &Case, limit: Duration, answer: impl FnOnce(&Path, &str, &str)) -> Vec<LeadEvent> {
    approvals::set_program(Some(std::env::current_exe().unwrap()));
    let file = case.file.clone();
    let spawner = move |spec: &LaunchSpec, _: &str| -> std::io::Result<o_core::sys::launch::Launch> {
        let at = spec.args.iter().position(|a| a == "--mcp-config").expect("normal mode asks O");
        assert_eq!(spec.args[spec.args.iter().position(|a| a == "--permission-prompt-tool").unwrap() + 1], approvals::prompt_tool());
        let config: Value = serde_json::from_str(&spec.args[at + 1]).unwrap();
        let args = config["mcpServers"][approvals::SERVER]["args"].to_string();
        let mut command = support::fake_worker_launch()?;
        command.args(["--exact", "fake_approving_cli", "--nocapture"]).env("O_TEST_APPROVE_ARGS", args).env("O_TEST_APPROVE_FILE", &file);
        Ok(command)
    };
    thread::scope(|scope| {
        let sending = scope.spawn(|| {
            let request = SendRequest { project: &case.project, provider: ProviderId::Claude, model: None, effort: None, text: "hello.txt를 만들어 줘", attach: &[], fresh: false, clean: None, yolo: false };
            lead::send_within(&case.home, &request, &spawner, limit).unwrap()
        });
        let deadline = Instant::now() + Duration::from_secs(20);
        let id = loop {
            assert!(Instant::now() < deadline, "no approval showed up");
            let shown = lead::show(&case.home, Some(&case.project));
            if let Some(id) = shown["thread"]["approvals"][0]["id"].as_str() {
                assert!(shown["thread"]["busy"].is_string());
                assert_eq!(shown["thread"]["approvals"][0]["files"][0]["added"], 2);
                assert!(shown["thread"]["approvals"][0].get("token").is_none());
                break id.to_string();
            }
            thread::sleep(Duration::from_millis(50));
        };
        let thread_id = lead::current_thread(&case.home, &case.project).unwrap();
        answer(&case.home, &thread_id, &id);
        sending.join().unwrap()
    })
}

fn thread_events(case: &Case) -> Vec<LeadEvent> {
    lead::read_thread(&case.home, &lead::current_thread(&case.home, &case.project).unwrap())
}

#[cfg_attr(not(windows), ignore = "the fake CLI runs as a Windows job like the real lead")]
#[test]
fn an_allowed_write_runs_and_its_step_counts_the_lines() {
    let case = case("allow");
    let added = turn(&case, Duration::from_secs(60), |home, _, id| lead::decide(home, id, true).unwrap());
    assert_eq!(fs::read_to_string(&case.file).unwrap(), "hi\nthere\n");
    assert!(added.iter().any(|e| matches!(e, LeadEvent::Step { kind, added: Some(2), removed: Some(0), status, .. } if kind == "edit" && status == "done")), "{added:?}");
    assert!(added.iter().any(|e| matches!(e, LeadEvent::Reply { text, .. } if text == "결과: allow")));
    let all = thread_events(&case);
    assert!(all.iter().any(|e| matches!(e, LeadEvent::Approval { tool, .. } if tool == "Write")));
    assert!(all.iter().any(|e| matches!(e, LeadEvent::Decision { decision, by, .. } if decision == "allow" && by == "user")));
    let shown = lead::show(&case.home, Some(&case.project));
    assert!(shown["thread"]["approvals"].as_array().unwrap().is_empty());
}

#[cfg_attr(not(windows), ignore = "the fake CLI runs as a Windows job like the real lead")]
#[test]
fn a_denied_write_never_runs() {
    let case = case("deny");
    let added = turn(&case, Duration::from_secs(60), |home, _, id| {
        lead::decide(home, id, false).unwrap();
        assert!(lead::decide(home, id, true).is_err(), "one answer only");
    });
    assert!(!case.file.exists());
    assert!(added.iter().any(|e| matches!(e, LeadEvent::Step { status, .. } if status == "denied")), "{added:?}");
    assert!(thread_events(&case).iter().any(|e| matches!(e, LeadEvent::Decision { decision, .. } if decision == "deny")));
}

/// The turn's time limit does not run while the person decides.
#[cfg_attr(not(windows), ignore = "the fake CLI runs as a Windows job like the real lead")]
#[test]
fn waiting_for_the_person_does_not_use_up_the_turn_limit() {
    let case = case("pause");
    let added = turn(&case, Duration::from_secs(3), |home, _, id| {
        thread::sleep(Duration::from_secs(5));
        lead::decide(home, id, true).unwrap();
    });
    assert!(case.file.exists(), "{added:?}");
    assert!(!added.iter().any(|e| matches!(e, LeadEvent::Error { text, .. } if text.contains("제한 시간"))), "{added:?}");
}

#[cfg_attr(not(windows), ignore = "the fake CLI runs as a Windows job like the real lead")]
#[test]
fn stop_refuses_what_waits_and_ends_the_turn() {
    let case = case("stop");
    let added = turn(&case, Duration::from_secs(60), |home, thread_id, id| {
        assert!(lead::stop(home, Some(&case.project)));
        // The request ends `stopped` (or closes with the turn); then it takes no answer.
        let deadline = Instant::now() + Duration::from_secs(10);
        while approvals::waiting(&approvals::folder(home, thread_id)) { assert!(Instant::now() < deadline); thread::sleep(Duration::from_millis(20)); }
        assert!(lead::decide(home, id, true).is_err());
    });
    assert!(!case.file.exists());
    assert!(added.iter().any(|e| matches!(e, LeadEvent::Error { text, .. } if text.contains("중지"))), "{added:?}");
    assert!(thread_events(&case).iter().any(|e| matches!(e, LeadEvent::Decision { decision, .. } if decision == "stopped")));
    assert!(!approvals::waiting(&approvals::folder(&case.home, &lead::current_thread(&case.home, &case.project).unwrap())));
}
