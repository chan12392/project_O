use std::process::Command;

#[test]
fn lead_monitor_stops_when_its_parent_closes_stdin() {
    let isolated = std::env::temp_dir().join(format!("o-monitor-test-{}", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_o"))
        .arg("_lead-watch")
        .env("LOCALAPPDATA", isolated)
        .env_remove("O_AGENT_ROLE").env_remove("O_CARD_ID")
        .stdin(std::process::Stdio::null()).output().unwrap();
    assert!(output.status.success());
}

#[test]
fn marked_agents_cannot_start_work_through_cli() {
    for role in ["lead", "card"] {
        for args in [
            vec!["dispatch"], vec!["followup", "run", "card"], vec!["_run", "run"], vec!["_lead-watch"], vec!["_followup", "run"], vec!["review", "1"], vec!["_review", "1"],
            vec!["lead", "send"], vec!["lead", "dispatch", "plan"], vec!["lead", "note", "thread", "-"], vec!["lead", "wake", "run"], vec!["lead", "retry", "run", "message"],
            // An agent never approves its own action (ADR-0008 개정 3).
            vec!["lead", "decide", "20261104-000000-abcd-a1", "allow"],
            vec!["schedule", "run", "test"], vec!["image", "read"], vec!["projects", "open", "x"],
            vec!["issues", "record", "finding.json"], vec!["issues", "classify", "1", "external"], vec!["issues", "resolve", "1", "run", "card", "evidence"], vec!["issues", "reopen", "1"],
        ] {
            let (key, value) = if role == "card" { ("O_CARD_ID", "card-1") } else { ("O_AGENT_ROLE", "lead") };
            let output = Command::new(env!("CARGO_BIN_EXE_o")).args(&args).env(key, value).output().unwrap();
            assert_eq!(output.status.code(), Some(2), "{role} {args:?}");
            assert!(String::from_utf8_lossy(&output.stderr).contains("에이전트 안에서는"));
        }
    }
}

#[test]
fn empty_agent_markers_do_not_hide_normal_argument_errors() {
    let output = Command::new(env!("CARGO_BIN_EXE_o")).arg("review").env("O_AGENT_ROLE", "").env("O_CARD_ID", "").output().unwrap();
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn marked_agent_hook_blocks_dispatch_command() {
    use std::io::Write;
    use std::process::Stdio;
    for command in ["o dispatch plan.json", "o followup run card", "o _followup run", "o lead decide 20261104-000000-abcd-a1 allow"] {
    let mut child = Command::new(env!("CARGO_BIN_EXE_o"))
        .arg("hook").env("O_AGENT_ROLE", "lead").stdin(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(serde_json::json!({"tool_name":"Bash","tool_input":{"command":command}}).to_string().as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("에이전트 안에서는"));
    }
}

/// The approval server refuses what O's guard rules refuse before anyone is asked, and an
/// MCP tool of its own, and answers Claude Code's handshake (`o _lead-approve`).
#[test]
fn the_approval_server_refuses_guarded_calls_without_asking() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::Stdio;
    let base = std::env::temp_dir().join(format!("o-approve-cli-{}", std::process::id()));
    let home = base.join("state");
    let project = base.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let project = project.to_string_lossy().into_owned();
    let thread = o_core::lead::thread_for(&home, &project, false).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_o"))
        .args(["_lead-approve", &home.to_string_lossy(), &thread, "token", &project])
        .env("O_AGENT_ROLE", "lead").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut ask = |id: u32, method: &str, params: serde_json::Value| -> serde_json::Value {
        writeln!(stdin, "{}", serde_json::json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})).unwrap();
        serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap()
    };
    assert_eq!(ask(0, "initialize", serde_json::json!({"protocolVersion":"2025-11-25"}))["result"]["serverInfo"]["name"], "o_lead");
    for (id, tool, input) in [(1, "Bash", serde_json::json!({"command":"git push origin main"})), (2, "mcp__o_lead__decide", serde_json::json!({}))] {
        let reply = ask(id, "tools/call", serde_json::json!({"name":"decide","arguments":{"tool_name":tool,"input":input}}));
        let decision: serde_json::Value = serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(decision["behavior"], "deny", "{tool}");
        assert!(decision["message"].as_str().unwrap().contains("O 정책"), "{decision}");
    }
    drop(stdin);
    assert!(child.wait().unwrap().success());
    let events: Vec<o_core::lead::LeadEvent> = o_core::lead::read_thread(&home, &thread);
    assert_eq!(events.iter().filter(|e| matches!(e, o_core::lead::LeadEvent::Decision { by, .. } if by == "guard")).count(), 2);
}
