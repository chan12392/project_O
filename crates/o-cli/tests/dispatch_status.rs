use std::{fs, process::Command};

#[test]
fn status_shows_the_complete_isolated_workspace_path() {
    let base = std::env::temp_dir().join(format!("o-status-workspace-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap()));
    fs::create_dir(&base).unwrap();
    let run = base.join("project_O/runs/run-1");
    fs::create_dir_all(&run).unwrap();
    let path = format!("C:/work/{}/result", "long-name-".repeat(20));
    let events = [
        serde_json::json!({"t":"run.start","at":"2026-01-01T00:00:00Z","runId":"run-1","title":"task","project":"C:/work","cards":[{"id":"a","provider":"claude"}]}),
        serde_json::json!({"t":"card.tool","at":"2026-01-01T00:00:01Z","cardId":"a","name":"O 작업 폴더","summary":path}),
        serde_json::json!({"t":"card.end","at":"2026-01-01T00:00:02Z","cardId":"a","status":"done","result":"done"}),
    ];
    fs::write(run.join("events.jsonl"), events.iter().map(|event| format!("{event}\n")).collect::<String>()).unwrap();
    fs::write(run.join("workspace-baseline.json"), serde_json::json!({"gitCommit":"0123456789abcdef"}).to_string()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_o")).args(["status", "run-1"]).env("LOCALAPPDATA", &base).env_remove("O_AGENT_ROLE").env_remove("O_CARD_ID").output().unwrap();
    assert!(output.status.success());
    let output = String::from_utf8_lossy(&output.stdout);
    assert!(output.contains(&format!("작업 폴더: {path}")));
    assert!(output.contains("작업 전 Git 기준: 0123456789abcdef"));
}

#[cfg(windows)]
#[test]
fn dispatch_creates_isolated_copy_before_starting_the_fake_cli() {
    use std::{io::Write, process::Stdio, time::{Duration, Instant}};
    let base = std::env::temp_dir().join(format!("o-cli-dispatch-isolated-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap()));
    fs::create_dir(&base).unwrap();
    assert!(base.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
    assert!(base.ancestors().all(|path| !path.join(".git").exists()));
    let project = base.join("project");
    let fake_bin = base.join("bin");
    fs::create_dir(&project).unwrap();
    fs::create_dir(&fake_bin).unwrap();
    fs::write(project.join("original.txt"), "unchanged").unwrap();
    fs::copy(std::env::current_exe().unwrap(), fake_bin.join("claude.exe")).unwrap();
    let cli = std::path::Path::new(env!("CARGO_BIN_EXE_o"));
    assert!(["claude.exe", "claude.cmd", "claude.bat"].iter().all(|name| !cli.parent().unwrap().join(name).exists()));
    let path = std::env::join_paths(std::iter::once(fake_bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()))).unwrap();
    let plan = serde_json::json!({"title":"test", "project":project, "cards":[{"id":"a", "provider":"claude", "task":"edit"}]}).to_string();
    let dispatch = || {
        let mut process = Command::new(cli).args(["dispatch", "--profile", "own", "-"]).env("LOCALAPPDATA", &base).env("PATH", &path)
            .env_remove("O_AGENT_ROLE").env_remove("O_CARD_ID").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        process.stdin.take().unwrap().write_all(plan.as_bytes()).unwrap();
        process.wait_with_output().unwrap()
    };
    let nested = project.join("nested");
    fs::create_dir(&nested).unwrap();
    fs::create_dir(nested.join(".git")).unwrap();
    let refused = dispatch();
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("분리할 수 없습니다"));
    assert!(!base.join("project_O/runs").exists());
    fs::remove_dir(nested.join(".git")).unwrap();
    fs::remove_dir(&nested).unwrap();
    let output = dispatch();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["isolated"], true);
    let id = result["runId"].as_str().unwrap();
    let events = base.join("project_O/runs").join(id).join("events.jsonl");
    let until = Instant::now() + Duration::from_secs(20);
    let text = loop {
        let text = fs::read_to_string(&events).unwrap_or_default();
        if text.lines().any(|line| serde_json::from_str::<serde_json::Value>(line).is_ok_and(|value| value["t"] == "run.end")) { break text; }
        assert!(Instant::now() < until, "isolated dispatch did not finish");
        std::thread::sleep(Duration::from_millis(50));
    };
    let workspace = text.lines().filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|value| value["t"] == "card.tool" && value["name"] == "O 작업 폴더").unwrap();
    let path = std::path::Path::new(workspace["summary"].as_str().unwrap());
    assert!(path.canonicalize().unwrap().starts_with(base.join("project_O-worktrees").canonicalize().unwrap()));
    assert_eq!(fs::read_to_string(path.join("original.txt")).unwrap(), "unchanged");
    assert_eq!(fs::read_to_string(project.join("original.txt")).unwrap(), "unchanged");
    let status = Command::new(cli).args(["status", id]).env("LOCALAPPDATA", &base).env_remove("O_AGENT_ROLE").env_remove("O_CARD_ID").output().unwrap();
    assert!(status.status.success());
    let status = String::from_utf8_lossy(&status.stdout);
    let baseline: serde_json::Value = serde_json::from_slice(&fs::read(base.join("project_O/runs").join(id).join("workspace-baseline.json")).unwrap()).unwrap();
    let baseline = baseline["copyPath"].as_str().unwrap();
    assert_eq!(std::path::Path::new(baseline).canonicalize().unwrap(), base.join("project_O-worktrees").join(id).join("base").canonicalize().unwrap());
    assert!(status.contains(baseline));
    assert!(status.contains("자동 반영되지 않습니다"));
}
