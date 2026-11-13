use std::process::{Command, Stdio};

#[test]
fn agent_cannot_open_write_approve_or_close() {
    for verb in ["open", "write", "paste", "close"] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_o"));
        #[cfg(windows)]
        { use std::os::windows::process::CommandExt; command.creation_flags(0x0800_0000); }
        let output = command.args(["term", verb]).env("O_AGENT_ROLE", "card").stdin(Stdio::null()).output().unwrap();
        assert!(!output.status.success(), "{verb}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("에이전트"), "{verb}");
    }
}

#[cfg(windows)]
#[test]
fn read_temporary_output_failure_exits_nonzero() {
    use std::os::windows::process::CommandExt;
    let root = std::env::temp_dir().join(format!("project-o-term-read-cli-{}", o_core::run::store::new_run_id(chrono::Utc::now())));
    let id = "t-20261111-093000-ab12";
    let session = root.join("project_O").join("terminals").join(id);
    std::fs::create_dir_all(&session).unwrap();
    std::fs::write(session.join("session.json"), serde_json::to_vec(&serde_json::json!({"id":id,"provider":"test","project":"test","purpose":"lead","threadId":null,"runId":null,"cardId":null,"state":"exited","exitCode":0,"cols":80,"rows":24,"startedAt":"2026-11-11T00:00:00.000Z"})).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_o")).args(["term", "read", id, "--offset", "0", "--json"]).env("LOCALAPPDATA", root).creation_flags(0x0800_0000).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("다시 읽어 주세요"));
}

#[cfg(windows)]
#[test]
fn detached_lead_survives_cli_and_round_trips() {
    use std::{io::Write, os::windows::process::CommandExt, thread, time::{Duration, Instant}};
    let root = std::env::temp_dir().join(format!("project-o-term-cli-{}", o_core::run::store::new_run_id(chrono::Utc::now())));
    let bin = root.join("bin");
    let project = root.join("project");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let home = root.join("project_O");
    let thread_id = o_core::lead::new_thread(&home, &project.to_string_lossy()).unwrap();
    let cmd = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32").join("cmd.exe");
    std::fs::copy(cmd, bin.join("claude.exe")).unwrap();
    let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()))).unwrap();
    let command = || {
        let mut c = Command::new(env!("CARGO_BIN_EXE_o"));
        c.creation_flags(0x0800_0000).env("LOCALAPPDATA", &root).env("PATH", &path).env_remove("O_AGENT_ROLE").env_remove("O_CARD_ID");
        c
    };
    let opened = command().args(["term", "open", "--provider", "claude", "--project", project.to_str().unwrap(), "--thread", &thread_id, "--json"]).output().unwrap();
    assert!(opened.status.success(), "{}", String::from_utf8_lossy(&opened.stderr));
    let session: serde_json::Value = serde_json::from_slice(&opened.stdout).unwrap();
    let id = session["id"].as_str().unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    let agy_error = root.join("agy-error.txt");
    let paste_error = root.join("paste-error.txt");
    let commands = format!("\"{}\" agy --workers on --ack 2>\"{}\"\r\"{}\" term paste {} 2>\"{}\"\r", env!("CARGO_BIN_EXE_o"), agy_error.display(), env!("CARGO_BIN_EXE_o"), id, paste_error.display());
    o_core::term::write(&home, id, commands.as_bytes()).unwrap();
    while std::fs::metadata(&agy_error).map_or(true, |m| m.len() == 0) || std::fs::metadata(&paste_error).map_or(true, |m| m.len() == 0) { assert!(Instant::now() < deadline, "lead child did not run boundary checks"); thread::sleep(Duration::from_millis(30)); }
    assert!(std::fs::read_to_string(&agy_error).unwrap().contains("에이전트"));
    assert!(std::fs::read_to_string(&paste_error).unwrap().contains("에이전트"));
    assert!(!o_core::agents::agy::workers_allowed(&home));
    let plans = o_core::lead::files::thread_files(&home, &thread_id).join("plans");
    while !plans.is_dir() { assert!(Instant::now() < deadline, "plan watcher did not start"); thread::sleep(Duration::from_millis(30)); }
    std::fs::write(plans.join("plan.json"), br#"{"title":"test","cards":[{"id":"a","provider":"claude","task":"work"}]}"#).unwrap();
    std::fs::write(plans.join("invalid.json"), b"{").unwrap();
    loop {
        let events = o_core::lead::read_thread(&home, &thread_id);
        let plans = events.iter().filter(|event| matches!(event, o_core::lead::LeadEvent::Plan { .. })).count();
        let errors = events.iter().filter(|event| matches!(event, o_core::lead::LeadEvent::Error { .. })).count();
        if plans == 1 && errors == 1 { break; }
        assert!(Instant::now() < deadline, "plan events not recorded");
        thread::sleep(Duration::from_millis(30));
    }
    let mut child = command().args(["term", "write", id, "-"]).stdin(Stdio::piped()).stdout(Stdio::null()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(b"echo SESSIONMARK\r").unwrap();
    assert!(child.wait().unwrap().success());
    assert!(command().args(["term", "resize", id, "91", "28"]).status().unwrap().success());
    loop {
        let output = command().args(["term", "read", id, "--offset", "0", "--json"]).output().unwrap();
        assert!(output.status.success());
        let chunk: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let bytes = o_core::term::decode_data(chunk["data"].as_str().unwrap()).unwrap();
        if String::from_utf8_lossy(&bytes).contains("SESSIONMARK") { break; }
        assert!(Instant::now() < deadline, "no terminal echo");
        thread::sleep(Duration::from_millis(30));
    }
    assert!(command().args(["term", "close", id]).status().unwrap().success());
    loop {
        let output = command().args(["term", "list", "--json"]).output().unwrap();
        let sessions: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        if sessions[0]["state"] == "exited" { break; }
        assert!(Instant::now() < deadline, "terminal did not exit");
        thread::sleep(Duration::from_millis(30));
    }
}
