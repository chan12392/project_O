use std::{fs, io::{Read, Write}, path::Path, process::{Command, Stdio}, thread, time::{Duration, Instant}};
use chrono::Utc;
use o_core::run::{events::OEvent, input::{self, RunInput}, mode::{Profile, RunMode}, plan::Plan, runner, store};
use o_core::sys::proc;
use serde_json::{json, Value};

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    #[cfg(windows)]
    if args.first().is_some_and(|arg| arg == "--o-handle-owner") {
        o_core::sys::win_launch::run_handle_owner(&args[1..].iter().map(Into::into).collect::<Vec<_>>()).unwrap();
        return;
    }
    #[cfg(windows)]
    if args.first().is_some_and(|arg| arg == "_job-watch") {
        std::process::exit(i32::from(o_core::run::job_watch::run(&args[1..]).is_err()));
    }
    #[cfg(target_os = "macos")]
    if args.first().is_some_and(|arg| arg == "--o-mac-worker") {
        std::process::exit(i32::from(o_core::sys::macos_worker::run_helper(&args[1..].iter().map(Into::into).collect::<Vec<_>>()).is_err()));
    }
    if args.first().is_some_and(|arg| arg == "_fixture-run") {
        let approved = input::receive(Path::new(&args[1]), &args[2], false).unwrap();
        if let Err(error) = runner::run_detached(&approved) { panic!("{error}"); }
        return;
    }
    let exe = std::env::current_exe().unwrap();
    if exe.file_stem().is_some_and(|name| name == "claude") {
        if args == ["--version"] { println!("2.1.300 (Claude Code)"); }
        else if args == ["auth", "status"] { println!("{}", json!({"loggedIn":true})); }
        else {
            let mut prompt = String::new();
            std::io::stdin().read_to_string(&mut prompt).unwrap();
            fs::write(std::env::var_os("O_FIXTURE_LAUNCH").unwrap(), json!({"args":args,"prompt":prompt,"cwd":std::env::current_dir().unwrap(),"profile":std::env::var("CLAUDE_CONFIG_DIR").ok()}).to_string()).unwrap();
            println!("{}", json!({"type":"system","subtype":"init","session_id":"fixture-session"}));
            println!("{}", json!({"type":"result","subtype":"success","is_error":false,"result":"Fixture complete"}));
        }
        return;
    }
    assert!(args.is_empty(), "unexpected fixture command: {args:?}");
    let root = std::env::temp_dir().join(format!("o-execution-input-{}-{}", std::process::id(), Utc::now().timestamp_nanos_opt().unwrap()));
    let bin = root.join("bin");
    let project = root.join("project");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("source.txt"), "approved source").unwrap();
    let provider = format!("claude{}", std::env::consts::EXE_SUFFIX);
    assert!(!Path::new(env!("CARGO_BIN_EXE_o")).parent().unwrap().join(&provider).exists());
    fs::copy(&exe, bin.join(provider)).unwrap();
    let path = std::env::join_paths(std::iter::once(bin.clone()).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()))).unwrap();
    unsafe {
        std::env::set_var("PATH", path);
        std::env::set_var("LOCALAPPDATA", root.join("local"));
        std::env::set_var("XDG_DATA_HOME", root.join("local"));
        std::env::set_var("CLAUDE_CONFIG_DIR", root.join("own-profile"));
        std::env::set_var("CODEX_HOME", root.join("codex-profile"));
        std::env::set_var("O_FIXTURE_LAUNCH", root.join("launch.json"));
    }
    assert!(o_core::sys::resolve::resolve_cli("claude").unwrap().command.starts_with(&bin));
    let home = store::default_home();
    assert!(home.starts_with(&root), "fixture must not use operational state: {}", home.display());
    let plan: Plan = serde_json::from_value(json!({"title":"Approved","project":project,"cards":[{"id":"a","provider":"claude","model":"sonnet","effort":"medium","task":"approved task"}]})).unwrap();
    let mode = RunMode { profile:Profile::Own, context:false, yolo:false };
    let approved = RunInput::new(&home, "immutable", &plan, mode, true, true);
    runner::prepare_isolated(&home, approved.id(), &plan, &mode).unwrap();
    let run = store::run_dir(&home, approved.id());
    fs::write(run.join("plan.json"), json!({"title":"Tampered","project":project,"cards":[{"id":"a","provider":"agy","model":"tampered","task":"unapproved task"}]}).to_string()).unwrap();
    for marker in ["clean", "yolo"] { fs::write(run.join(marker), "").unwrap(); }
    for marker in ["isolated-worktrees", "no-context", "scheduled"] { let _ = fs::remove_file(run.join(marker)); }
    input::spawn(Command::new(&exe).arg("_fixture-run").arg(&home).arg(approved.id()), &approved).expect("trusted inherited input");
    wait_done(&home, approved.id());
    let launched: Value = serde_json::from_slice(&fs::read(root.join("launch.json")).unwrap()).unwrap();
    let arguments = launched["args"].as_array().unwrap();
    assert!(arguments.iter().any(|arg| arg == "sonnet"));
    assert!(arguments.iter().any(|arg| arg == "medium"));
    assert!(launched["prompt"].as_str().is_some_and(|text| text.contains("approved task") && !text.contains("unapproved task")));
    assert!(!arguments.iter().any(|arg| arg.as_str().is_some_and(|arg| arg.contains("dangerously-skip"))));
    assert_ne!(launched["cwd"], json!(project));
    assert_eq!(launched["profile"], json!(root.join("own-profile")));
    let events = store::read_event_values(&home, approved.id());
    assert!(events.iter().any(|e| e["t"] == "run.mode" && e["yolo"] == false && e["profile"] == "own" && e["context"] == false && e["scheduled"] == true), "{events:?}");
    assert!(!events.iter().any(|e| e["t"] == "run.context"));

    for command in ["_run", "_run-approved", "_followup", "_followup-approved"] {
        let mut child = proc::hide_window(Command::new(env!("CARGO_BIN_EXE_o")).args([command, approved.id()])).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(&serde_json::to_vec(&approved).unwrap()).unwrap();
        let result = child.wait_with_output().unwrap();
        assert!(!result.status.success(), "forged input must fail: {command}");
        assert!(String::from_utf8_lossy(&result.stderr).contains("파이프"), "{}", String::from_utf8_lossy(&result.stderr));
    }
    let missing = proc::hide_window(Command::new(env!("CARGO_BIN_EXE_o")).args(["_run", "missing"])).stdin(Stdio::null()).output().unwrap();
    assert!(!missing.status.success());
    assert!(!store::run_dir(&home, "missing").exists());

    let path = root.join("plan.json");
    fs::write(&path, serde_json::to_vec(&plan).unwrap()).unwrap();
    let cli = proc::hide_window(Command::new(env!("CARGO_BIN_EXE_o")).arg("dispatch").arg(path).args(["--profile", "own", "--no-context", "--mode", "normal"])).output().unwrap();
    assert!(cli.status.success(), "{}", String::from_utf8_lossy(&cli.stderr));
    let dispatched: Value = serde_json::from_slice(&cli.stdout).unwrap();
    wait_done(&home, dispatched["runId"].as_str().unwrap());

    let thread = o_core::lead::new_thread(&home, plan.project.as_str()).unwrap();
    let plan_id = format!("{thread}-p1");
    let expected = serde_json::to_value(&plan).unwrap();
    let mut replaced = expected.clone();
    replaced["cards"][0]["task"] = json!("changed after display");
    let event = o_core::lead::LeadEvent::Plan { at: Utc::now().to_rfc3339(), plan_id: plan_id.clone(), plan: replaced.clone(), ok:true, violations:vec![], notices:vec![] };
    writeln!(fs::OpenOptions::new().append(true).open(o_core::lead::thread_dir(&home, &thread).join("events.jsonl")).unwrap(), "{}", serde_json::to_string(&event).unwrap()).unwrap();
    let denied = o_core::lead::dispatch(&home, &plan_id, mode, Some(&expected), &|_, _| panic!("a changed plan must not launch"));
    assert!(matches!(denied, Err(o_core::lead::DispatchError::Other(text)) if text.contains("계획과 현재 계획이 다릅니다")));
    let mut stale_mode = proc::hide_window(Command::new(env!("CARGO_BIN_EXE_o")).args(["lead", "dispatch", &plan_id, "--expected-plan", "--profile", "own", "--mode", "yolo"]))
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    stale_mode.stdin.take().unwrap().write_all(&serde_json::to_vec(&replaced).unwrap()).unwrap();
    let stale_mode = stale_mode.wait_with_output().unwrap();
    assert!(!stale_mode.status.success());
    assert!(String::from_utf8_lossy(&stale_mode.stderr).contains("실행 모드와 현재 설정이 다릅니다"));
    let captured = std::sync::Mutex::new(None);
    o_core::lead::dispatch(&home, &plan_id, mode, Some(&replaced), &|_, input| { *captured.lock().unwrap() = Some(input.clone()); Ok(()) }).ok().unwrap();
    let captured = captured.into_inner().unwrap().unwrap();
    assert_eq!(captured.plan().cards[0].task, "changed after display");
    assert!(!captured.mode().yolo && !captured.mode().clean() && !captured.mode().context);
    let thread = o_core::lead::new_thread(&home, plan.project.as_str()).unwrap();
    let plan_id = format!("{thread}-p1");
    replaced["project"] = json!(root.join("different-project"));
    let event = o_core::lead::LeadEvent::Plan { at: Utc::now().to_rfc3339(), plan_id: plan_id.clone(), plan: replaced.clone(), ok:true, violations:vec![], notices:vec![] };
    writeln!(fs::OpenOptions::new().append(true).open(o_core::lead::thread_dir(&home, &thread).join("events.jsonl")).unwrap(), "{}", serde_json::to_string(&event).unwrap()).unwrap();
    let denied = o_core::lead::dispatch(&home, &plan_id, mode, Some(&replaced), &|_, _| panic!("thread metadata must not replace the approved project"));
    assert!(matches!(denied, Err(o_core::lead::DispatchError::Other(text)) if text.contains("프로젝트와 대화의 프로젝트가 다릅니다")));
    println!("execution integrity: frozen plan/modes/isolation, authenticated pipe, 5 direct-call refusals, human CLI and displayed-plan matching passed");
}

fn wait_done(home: &Path, id: &str) {
    let deadline = Instant::now() + Duration::from_secs(35);
    loop {
        let events = store::read_events(home, id);
        if let Some(status) = events.iter().find_map(|event| if let OEvent::RunEnd { status, .. } = event { Some(status) } else { None }) {
            assert_eq!(*status, o_core::run::events::CardStatus::Done, "{events:?}");
            return;
        }
        assert!(Instant::now() < deadline, "runner did not finish: {events:?}");
        thread::sleep(Duration::from_millis(25));
    }
}
