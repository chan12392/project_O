use std::process::Command;

#[test]
fn marked_agents_cannot_start_work_through_cli() {
    for role in ["lead", "card"] {
        for args in [
            vec!["dispatch"], vec!["followup", "run", "card"], vec!["_run", "run"], vec!["_followup", "run"], vec!["review", "1"], vec!["_review", "1"],
            vec!["lead", "send"], vec!["lead", "dispatch", "plan"], vec!["lead", "note", "thread", "-"],
            vec!["schedule", "run", "test"], vec!["image", "read"], vec!["projects", "open", "x"],
            vec!["issues", "record", "finding.json"], vec!["issues", "classify", "1", "external"],
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
    for command in ["o dispatch plan.json", "o followup run card", "o _followup run"] {
    let mut child = Command::new(env!("CARGO_BIN_EXE_o"))
        .arg("hook").env("O_AGENT_ROLE", "lead").stdin(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(serde_json::json!({"tool_name":"Bash","tool_input":{"command":command}}).to_string().as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("에이전트 안에서는"));
    }
}
