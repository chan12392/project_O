use std::{fs, io::Write, path::Path, process::Command, sync::Mutex};

use chrono::Utc;
use o_core::{lead::{self, LeadEvent}, run::{events::{CardStatus, OEvent}, mode::{Profile, RunMode}, plan::ProviderId, runner, store}, sys::proc, usage::fallback};
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
    let exe = std::env::current_exe().unwrap();
    let name = exe.file_stem().unwrap().to_string_lossy();
    if name == "claude" || name == "codex" {
        if args == ["--version"] { println!("{}", if name == "claude" { "2.1.100 (Claude Code)" } else { "codex-cli 0.166.0" }); }
        else if args == ["auth", "status"] { println!("{}", json!({"loggedIn":std::env::var("O_FIXTURE_SIGNED_OUT").unwrap_or_default() != name})); }
        else if args == ["login", "status"] {
            if std::env::var("O_FIXTURE_SIGNED_OUT").unwrap_or_default() == name { println!("Not logged in"); std::process::exit(1); }
            println!("Logged in using ChatGPT");
        }
        else if args == ["debug", "models"] { println!("{}", json!({"models":[{"slug":"target-model","display_name":"Fixture","visibility":"list","supported_reasoning_levels":[{"effort":"medium"}]}]})); }
        else if name == "codex" && args.first().is_some_and(|arg| arg == "exec") {
            fs::write(std::env::var_os("O_FIXTURE_LAUNCH").unwrap(), json!({"args":args,"cwd":std::env::current_dir().unwrap()}).to_string()).unwrap();
            println!("{}", json!({"type":"thread.started","thread_id":"fixture-destination"}));
            println!("{}", json!({"type":"item.completed","item":{"type":"agent_message","text":"Fixture complete"}}));
            println!("{}", json!({"type":"turn.completed","usage":{"input_tokens":9,"cached_input_tokens":3,"output_tokens":1}}));
        } else { panic!("unexpected fixture provider invocation: {name} {args:?}"); }
        return;
    }

    assert!(args.is_empty(), "unexpected fixture harness invocation: {args:?}");
    let base = std::env::temp_dir().join(format!("o-fallback-dispatch-{}-{}", std::process::id(), Utc::now().timestamp_nanos_opt().unwrap()));
    let bin = base.join("bin");
    fs::create_dir_all(&bin).unwrap();
    for provider in ["claude", "codex"] {
        let filename = format!("{provider}{}", std::env::consts::EXE_SUFFIX);
        // o puts its own folder first; this fixture must never discover a real provider there.
        assert!(!Path::new(env!("CARGO_BIN_EXE_o")).parent().unwrap().join(&filename).exists());
        fs::copy(&exe, bin.join(filename)).unwrap();
    }
    let path = std::env::join_paths(std::iter::once(bin.clone()).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()))).unwrap();
    unsafe {
        std::env::set_var("PATH", path);
        std::env::set_var("CODEX_HOME", base.join("codex-profile"));
        std::env::set_var("CLAUDE_CONFIG_DIR", base.join("claude-profile"));
        std::env::set_var("O_FIXTURE_LAUNCH", base.join("launch.json"));
    }
    for provider in ["claude", "codex"] { assert!(o_core::sys::resolve::resolve_cli(provider).unwrap().command.starts_with(&bin)); }
    for scenario in ["success", "off", "generic-429", "invalid-model", "target-cooldown", "unsupported-effort", "protected-project", "same-provider", "source-login", "target-login", "late-off"] {
        exercise(&base, scenario);
    }
    println!("fallback dispatch: 11 CLI/lead/detached admission scenarios passed using fixture providers");
}

fn git(project: &Path, args: &[&str]) {
    let output = proc::hide_window(Command::new("git").current_dir(project).args(args)).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

fn exercise(base: &Path, scenario: &str) {
    unsafe { std::env::set_var("O_FIXTURE_SIGNED_OUT", match scenario { "source-login" => "claude", "target-login" => "codex", _ => "" }); }
    let dir = base.join(scenario);
    let home = dir.join("project_O");
    let project = dir.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("settings.json"), json!({"subscriptionFallback":{"enabled":scenario != "off","target":{"provider":if scenario == "same-provider" { "claude" } else { "codex" },"model":if scenario == "invalid-model" { "missing-model" } else { "target-model" }}}}).to_string()).unwrap();
    o_core::run::ram::write_enabled(&home, false).unwrap();
    if scenario == "generic-429" { store::set_cooldown(&home, ProviderId::Claude, "", Utc::now()).unwrap(); }
    else { fallback::record_limit(&home, ProviderId::Claude, "").unwrap(); }
    if scenario == "target-cooldown" { store::set_cooldown(&home, ProviderId::Codex, "", Utc::now()).unwrap(); }
    let input = json!({"title":"fixture","project":if scenario == "protected-project" { &home } else { &project },"cards":[{"id":"a","provider":"claude","task":"Read file.txt","model":"source-model","effort":if scenario == "unsupported-effort" { "high" } else { "medium" },"depends_on":[],"resources":[]}]});
    let input_path = dir.join("input.json");
    fs::write(&input_path, input.to_string()).unwrap();
    let accepted = matches!(scenario, "success" | "late-off");
    // o check and o dispatch execute this same admission function, before creating any run.
    let output = proc::hide_window(Command::new(env!("CARGO_BIN_EXE_o")).args(["check", "--profile", "own", "--json"]).arg(&input_path).env("LOCALAPPDATA", &dir)).output().unwrap();
    let result: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("CLI output: {} / {}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)));
    assert_eq!(output.status.success(), accepted, "{scenario}: {result}");
    let mode = RunMode { profile: Profile::Own, context: false, yolo: false };
    let checked_project = input["project"].as_str().unwrap();
    let (approved, violations, _) = lead::check_lead_plan(&home, checked_project, input.clone(), mode);
    assert_eq!(violations.is_empty(), accepted, "{scenario}: {violations:?}");
    assert_eq!(approved, input);
    if !accepted { return; }
    assert_eq!(result["plan"]["cards"][0]["provider"], "claude");
    let ctx = store::policy_context(&home, Utc::now()).unwrap();
    assert!(ctx.cooldowns.contains_key(&ProviderId::Claude));
    assert!(o_core::policy::check_plan_in(&input, &ctx, mode).plan.is_none());
    assert!(!home.join("fallback-attempts").exists());
    if scenario == "success" { check_policy_blockers(&home, &input, mode); }

    git(&project, &["init", "-b", "younchan"]);
    git(&project, &["config", "user.name", "Fixture"]);
    git(&project, &["config", "user.email", "fixture@example.invalid"]);
    fs::write(project.join("file.txt"), "unchanged\n").unwrap();
    git(&project, &["add", "file.txt"]);
    git(&project, &["commit", "-m", "test: fixture"]);
    let thread = lead::new_thread(&home, checked_project).unwrap();
    let plan_id = format!("{thread}-p1");
    let event = LeadEvent::Plan { at: Utc::now().to_rfc3339(), plan_id: plan_id.clone(), plan: input.clone(), ok: true, violations: vec![], notices: vec![] };
    writeln!(fs::OpenOptions::new().append(true).open(lead::thread_dir(&home, &thread).join("events.jsonl")).unwrap(), "{}", serde_json::to_string(&event).unwrap()).unwrap();
    let started = Mutex::new(String::new());
    lead::dispatch(&home, &plan_id, mode, &|run_id| { *started.lock().unwrap() = run_id.into(); Ok(()) })
        .unwrap_or_else(|error| match error { lead::DispatchError::Blocked(violations) => panic!("{violations:?}"), lead::DispatchError::Other(message) => panic!("{message}") });
    let run_id = started.into_inner().unwrap();
    let original = fs::read(store::run_dir(&home, &run_id).join("plan.json")).unwrap();
    assert_eq!(store::read_plan(&home, &run_id).unwrap()["cards"][0]["provider"], "claude");
    if scenario == "late-off" { fs::write(home.join("settings.json"), "{}").unwrap(); }
    let result = runner::run_detached(&home, &run_id);
    let events = store::read_events(&home, &run_id);
    assert_eq!(fs::read(store::run_dir(&home, &run_id).join("plan.json")).unwrap(), original);
    if scenario == "late-off" {
        assert!(result.is_err());
        assert!(!events.iter().any(|event| matches!(event, OEvent::CardFallback { .. } | OEvent::CardStart { .. })));
        return;
    }
    result.unwrap();
    assert!(events.iter().any(|event| matches!(event, OEvent::CardEnd { status: CardStatus::Done, .. })), "{events:?}");
    assert_eq!(events.iter().filter(|event| matches!(event, OEvent::CardFallback { from: ProviderId::Claude, from_model: Some(model), target, .. } if model == "source-model" && target.provider == ProviderId::Codex && target.model == "target-model")).count(), 1);
    let launch: Value = serde_json::from_slice(&fs::read(base.join("launch.json")).unwrap()).unwrap();
    let args = launch["args"].as_array().unwrap();
    assert!(args.windows(2).any(|pair| pair == [json!("-m"), json!("target-model")]));
    assert!(args.contains(&json!("model_reasoning_effort=medium")));
    assert!(launch["cwd"].as_str().unwrap().contains("project_O-worktrees"));
    assert!(!store::resource_held(&home, &run_id, "a").unwrap());
}

fn check_policy_blockers(home: &Path, input: &Value, mode: RunMode) {
    for provider in [ProviderId::Claude, ProviderId::Codex] {
        let mut ctx = store::policy_context(home, Utc::now()).unwrap();
        ctx.over_budget.insert(provider, "fixture daily budget".into());
        let checked = fallback::check_plan(home, input, &ctx, mode);
        assert!(checked.plan.is_none());
        assert!(checked.violations.iter().any(|v| v.rule == "daily-budget"));
        let mut ctx = store::policy_context(home, Utc::now()).unwrap();
        ctx.active_by_provider.insert(provider, o_core::policy::LIMITS.max_active_per_provider(provider));
        let checked = fallback::check_plan(home, input, &ctx, mode);
        assert!(checked.plan.is_none());
        assert!(checked.violations.iter().any(|v| v.rule == "active-cap"));
    }
    let mut ctx = store::policy_context(home, Utc::now()).unwrap();
    ctx.is_mirror = Box::new(|_| true);
    let checked = fallback::check_plan(home, input, &ctx, mode);
    assert!(checked.plan.is_none());
    assert!(checked.violations.iter().any(|v| v.rule != "provider-cooldown"));
    let mut ctx = store::policy_context(home, Utc::now()).unwrap();
    ctx.profile_not_ready = Box::new(|_| Some("fixture unready profile".into()));
    let checked = fallback::check_plan(home, input, &ctx, RunMode::of(Profile::ProjectO));
    assert!(checked.plan.is_none());
    assert!(checked.violations.iter().any(|v| v.rule == "profile-not-ready"));
}
