mod support;

use std::{fs, path::Path, process::Command, sync::{Arc, Mutex}};
use o_core::{run::{events::{CardStatus, OEvent}, plan::{Card, Plan, ProviderId}, mode::{Profile, RunMode}, runner::{self, Spawner}, store}, sys::proc};
use serde_json::json;

fn main() {
    let exe = std::env::current_exe().unwrap();
    let args: Vec<_> = std::env::args().skip(1).collect();
    let name = exe.file_stem().unwrap().to_string_lossy();
    if args == ["fixture-help"] { println!("fixture exec"); return; }
    if name == "claude" || name == "codex" {
        if args == ["--version"] { println!("{}", if name == "claude" { "2.1.100 (Claude Code)" } else { "codex-cli 0.166.0" }); }
        else if args == ["auth", "status"] { println!("{}", json!({"loggedIn":true})); }
        else if args == ["login", "status"] { println!("Logged in using ChatGPT"); }
        else if args == ["debug", "models"] { println!("{}", json!({"models":[{"slug":"target-model","display_name":"Target fixture","visibility":"list","supported_reasoning_levels":[{"effort":"medium"}]}]})); }
        else { panic!("unexpected metadata request: {args:?}"); }
        return;
    }
    if args.first().is_some_and(|a| a == "fixture-card") {
        if args[1] == "claude" {
            if args.get(2).is_some_and(|arg| arg == "unsafe-tool") { println!("{}", json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"tool","name":"Bash","input":{"command":"external action"}}]}})); }
            println!("{}", json!({"type":"system","subtype":"init","session_id":"source-session"}));
            println!("{}", json!({"type":"result","session_id":"source-session","subtype":"error","is_error":true,"result":"Claude AI usage limit reached","modelUsage":{"source-model":{"inputTokens":10,"cacheCreationInputTokens":0,"cacheReadInputTokens":5,"outputTokens":2}}}));
        } else {
            println!("{}", json!({"type":"thread.started","thread_id":"target-session"}));
            println!("{}", json!({"type":"item.completed","item":{"type":"agent_message","text":"Fixture complete"}}));
            println!("{}", json!({"type":"turn.completed","usage":{"input_tokens":9,"cached_input_tokens":3,"output_tokens":1}}));
            if args.get(2).is_some_and(|arg| arg == "target-quota") { println!("{}", json!({"type":"error","error":{"type":"usage_limit_reached","message":"usage limit reached"}})); std::process::exit(1); }
        }
        return;
    }
    let base = std::env::temp_dir().join(format!("o-fallback-runner-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap()));
    let bin = base.join("bin");
    fs::create_dir_all(&bin).unwrap();
    for provider in ["claude", "codex"] { fs::copy(&exe, bin.join(format!("{provider}{}", std::env::consts::EXE_SUFFIX))).unwrap(); }
    let path = std::env::join_paths(std::iter::once(bin.clone()).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()))).unwrap();
    unsafe {
        std::env::set_var("PATH", path);
        std::env::set_var("CODEX_HOME", base.join("codex-profile"));
        std::env::set_var("CLAUDE_CONFIG_DIR", base.join("claude-profile"));
    }
    for provider in ["claude", "codex"] { assert!(o_core::sys::resolve::resolve_cli(provider).unwrap().command.starts_with(&bin)); }
    for isolated in [false, true] { exercise(&base, isolated, "success"); }
    for scenario in ["off", "late-off", "target-quota", "invalid-model", "unsafe-tool"] { exercise(&base, false, scenario); }
    for preblocked in [false, true] { lead_fallback(&base, preblocked); }
    println!("fallback runner: 7 card and 2 lead scenarios passed without provider calls");
}

fn git(project: &Path, args: &[&str]) {
    let output = proc::hide_window(Command::new("git").current_dir(project).args(args)).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

fn exercise(base: &Path, isolated: bool, scenario: &'static str) {
    let dir = base.join(format!("{scenario}-{isolated}"));
    let home = dir.join("state");
    let project = dir.join("project");
    fs::create_dir_all(&project).unwrap(); fs::create_dir_all(&home).unwrap();
    git(&project, &["init", "-b", "younchan"]);
    git(&project, &["config", "user.name", "Fixture"]);
    git(&project, &["config", "user.email", "fixture@example.invalid"]);
    fs::write(project.join("file.txt"), "unchanged\n").unwrap();
    git(&project, &["add", "file.txt"]); git(&project, &["commit", "-m", "test: fixture"]);
    fs::write(home.join("settings.json"), json!({"subscriptionFallback":{"enabled":scenario != "off","target":{"provider":"codex","model":if scenario == "invalid-model" { "unavailable-model" } else { "target-model" }}}}).to_string()).unwrap();
    o_core::run::ram::write_enabled(&home, false).unwrap();
    let plan = Plan { title: "fixture".into(), project: project.to_string_lossy().into_owned(), cards: vec![Card { id: "a".into(), provider: ProviderId::Claude, role: None, task: "Read file.txt".into(), model: Some("source-model".into()), effort: Some("medium".into()), depends_on: vec![], resources: vec![] }] };
    if isolated { runner::prepare_isolated(&home, "run", &plan, &RunMode { profile: Profile::Own, context: false, yolo: false }).unwrap(); }
    else { store::create_run(&home, "run", &plan).unwrap(); }
    let original = fs::read(store::run_dir(&home, "run").join("plan.json")).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let output = seen.clone();
    let fake_home = home.clone();
    let spawner: Arc<Spawner> = Arc::new(move |spec, cwd| {
        output.lock().unwrap().push((spec.bin.clone(), spec.args.clone(), cwd.to_string()));
        if scenario == "late-off" && spec.bin == "codex" { fs::write(fake_home.join("settings.json"), "{}")?; }
        let mut launch = support::fake_worker_launch()?;
        launch.current_dir(cwd).args(["fixture-card", spec.bin.as_str(), scenario]);
        Ok(launch)
    });
    assert_eq!(runner::run_plan(&home, "run", &plan, None, spawner).unwrap(), if scenario == "success" { CardStatus::Done } else { CardStatus::Failed });
    let seen = seen.lock().unwrap();
    let attempted = !matches!(scenario, "off" | "invalid-model" | "unsafe-tool");
    assert_eq!(seen.len(), if attempted { 2 } else { 1 });
    if attempted {
    assert_eq!((seen[0].0.as_str(), seen[1].0.as_str()), ("claude", "codex"));
    assert_eq!(seen[0].2, seen[1].2);
    assert_eq!(seen[0].2.contains("state-worktrees"), isolated);
    assert!(seen[1].1.windows(2).any(|w| w == ["-m", "target-model"]));
    assert!(seen[1].1.iter().any(|a| a == "model_reasoning_effort=medium"));
    assert!(!seen[1].1.iter().any(|a| a == "source-session"));
    }
    assert_eq!(fs::read(store::run_dir(&home, "run").join("plan.json")).unwrap(), original);
    assert!(!store::resource_held(&home, "run", "a").unwrap());
    let events = store::read_events(&home, "run");
    assert_eq!(events.iter().filter(|e| matches!(e, OEvent::CardFallback { .. })).count(), usize::from(attempted));
    let target_ran = attempted && scenario != "late-off";
    assert_eq!(events.iter().filter(|e| matches!(e, OEvent::CardUsageSettled { .. })).count(), if target_ran { 2 } else { 1 });
    let report = o_core::context::tokens::report(&home, &Default::default()).unwrap();
    assert_eq!(report.runs[0].tokens.total, if target_ran { 27 } else { 17 });
    assert_eq!(report.runs[0].cards[0].by_provider["claude"].total, 17);
    if target_ran { assert_eq!(report.runs[0].cards[0].by_provider["codex"].total, 10); }
    let next = o_core::run::followup::target(&home, "run", "a").unwrap();
    assert_eq!(next.card.provider, if attempted { ProviderId::Codex } else { ProviderId::Claude });
    if target_ran { assert_eq!(next.session.as_deref(), Some("target-session")); }
}

fn lead_fallback(base: &Path, preblocked: bool) {
    use o_core::lead::{self, LeadEvent, SendRequest};
    let dir = base.join(format!("lead-{preblocked}"));
    let home = dir.join("state"); let project = dir.join("project");
    fs::create_dir_all(&home).unwrap(); fs::create_dir_all(&project).unwrap();
    fs::write(home.join("settings.json"), json!({"subscriptionFallback":{"enabled":true,"target":{"provider":"codex","model":"target-model"}}}).to_string()).unwrap();
    if preblocked { o_core::usage::fallback::record_limit(&home, ProviderId::Claude, "").unwrap(); }
    let seen = Arc::new(Mutex::new(Vec::new()));
    let output = seen.clone();
    let spawner = move |spec: &o_core::providers::LaunchSpec, cwd: &str| {
        let mut launch = support::fake_worker_launch()?;
        launch.current_dir(cwd);
        if spec.args.iter().any(|arg| arg == "--help") { launch.arg("fixture-help"); }
        else { output.lock().unwrap().push(spec.bin.clone()); launch.args(["fixture-card", spec.bin.as_str()]); }
        Ok(launch)
    };
    let project = project.to_string_lossy();
    let req = SendRequest { project: &project, thread: None, provider: ProviderId::Claude, model: Some("source-model"), effort: Some("medium"), text: "Read the project", attach: &[], fresh: false, clean: None, yolo: false };
    let events = lead::send(&home, &req, &spawner).unwrap();
    assert_eq!(*seen.lock().unwrap(), if preblocked { vec!["codex"] } else { vec!["claude", "codex"] });
    assert_eq!(events.iter().filter(|e| matches!(e, LeadEvent::Fallback { .. })).count(), 1);
    assert_eq!(events.iter().filter(|e| matches!(e, LeadEvent::User { text, provider: ProviderId::Claude, .. } if text == req.text)).count(), 1);
    assert!(events.iter().any(|e| matches!(e, LeadEvent::Reply { text, .. } if text == "Fixture complete")));
    let total: f64 = events.iter().filter_map(|e| match e { LeadEvent::UsageSettled { usage, .. } => Some(usage.input + usage.cached + usage.output), _ => None }).sum();
    assert_eq!(total, if preblocked { 10.0 } else { 27.0 });
}
