use std::{fs, path::PathBuf};
use o_core::run::{mode::{Profile, RunMode}, plan::Plan, runner};
#[cfg(windows)]
use o_core::run::store;

fn fixture() -> (PathBuf, PathBuf, Plan) {
    let root = std::env::temp_dir().join(format!("o-dispatch-isolation-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap()));
    fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    assert!(root.starts_with(std::env::temp_dir().canonicalize().unwrap()));
    assert!(root.ancestors().all(|path| !path.join(".git").exists()), "test project must not be inside a Git repository");
    let project = root.join("project");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("result.txt"), "original").unwrap();
    let plan = serde_json::from_value(serde_json::json!({"title":"test", "project":project, "cards":[{"id":"a", "provider":"claude", "task":"edit"}]})).unwrap();
    (root.join("state"), project, plan)
}

#[test]
fn preparation_refuses_unsafe_run_ids_without_creating_state() {
    let (home, _, plan) = fixture();
    assert!(runner::prepare_isolated(&home, "../bad", &plan, &RunMode::of(Profile::Own)).is_err());
    assert!(!home.exists());
}

#[test]
fn preparation_refuses_a_hard_link_without_creating_a_run() {
    let (home, project, plan) = fixture();
    let outside = home.parent().unwrap().join("outside.txt");
    fs::write(&outside, "private").unwrap();
    fs::hard_link(&outside, project.join("linked.txt")).unwrap();
    let error = runner::prepare_isolated(&home, "run-1", &plan, &RunMode::of(Profile::Own)).unwrap_err();
    assert!(error.to_string().contains("링크된 파일"));
    assert!(!home.exists());
    assert_eq!(fs::read_to_string(outside).unwrap(), "private");
}

#[cfg(windows)]
#[test]
fn fake_card() {
    if std::env::var_os("O_DISPATCH_ISOLATION_FAKE").is_none() { return; }
    fs::write("result.txt", "worker result").unwrap();
    println!("{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"done\"}}");
}

#[cfg(windows)]
#[test]
fn normal_dispatch_preparation_runs_a_card_in_an_isolated_non_git_copy() {
    use std::sync::Arc;
    use o_core::run::events::{CardStatus, OEvent};
    let (home, project, plan) = fixture();
    runner::prepare_isolated(&home, "run-1", &plan, &RunMode::of(Profile::Own)).unwrap();
    assert!(store::run_dir(&home, "run-1").join("isolated-worktrees").is_file());
    let spawner: Arc<runner::Spawner> = Arc::new(|_, cwd| {
        let mut command = support::fake_worker_launch()?;
        command.current_dir(cwd).args(["--exact", "fake_card", "--nocapture"]).env("O_DISPATCH_ISOLATION_FAKE", "1");
        Ok(command)
    });
    assert_eq!(runner::run_plan(&home, "run-1", &plan, None, spawner).unwrap(), CardStatus::Done);
    assert_eq!(fs::read_to_string(project.join("result.txt")).unwrap(), "original");
    let baseline: serde_json::Value = serde_json::from_slice(&fs::read(store::run_dir(&home, "run-1").join("workspace-baseline.json")).unwrap()).unwrap();
    assert_eq!(baseline["copySha256"].as_str().unwrap().len(), 64);
    let baseline = PathBuf::from(baseline["copyPath"].as_str().unwrap());
    assert_eq!(fs::read_to_string(baseline.join("result.txt")).unwrap(), "original");
    let events = store::read_events(&home, "run-1");
    let workspace = events.iter().find_map(|event| match event {
        OEvent::CardTool { card_id, name, summary, .. } if card_id == "a" && name == "O 작업 폴더" => Some(PathBuf::from(summary)),
        _ => None,
    }).expect("isolated workspace must be visible in the run");
    assert_eq!(fs::read_to_string(workspace.join("result.txt")).unwrap(), "worker result");
    assert!(workspace.canonicalize().unwrap().starts_with(home.with_file_name("state-worktrees").canonicalize().unwrap()));
    store::append_event(&home, "run-1", &OEvent::CardSession { at: "2026-01-01T00:00:00Z".into(), card_id: "a".into(), session_id: "session-1".into() }).unwrap();
    store::create_run(&home, "follow-1", &plan).unwrap();
    fs::write(store::run_dir(&home, "follow-1").join("followup.json"), r#"["run-1","a"]"#).unwrap();
    store::append_event(&home, "follow-1", &OEvent::CardTool { at: "2026-01-01T00:00:01Z".into(), card_id: "a".into(), name: "O 작업 폴더".into(), summary: workspace.to_string_lossy().into_owned() }).unwrap();
    let initial = o_core::run::review_changes(&home, "run-1", "a").unwrap();
    let continued = o_core::run::review_changes(&home, "follow-1", "a").unwrap();
    assert_eq!(continued["baselineRunId"], "run-1");
    assert_eq!(continued["changes"], initial["changes"]);
    store::append_event(&home, "follow-1", &OEvent::CardSession { at: "2026-01-01T00:00:02Z".into(), card_id: "a".into(), session_id: "session-1".into() }).unwrap();
    store::append_event(&home, "follow-1", &OEvent::CardEnd { at: "2026-01-01T00:00:03Z".into(), card_id: "a".into(), status: CardStatus::Done, result: None, exit_code: Some(0) }).unwrap();
    store::create_run(&home, "follow-2", &plan).unwrap();
    fs::write(store::run_dir(&home, "follow-2").join("followup.json"), r#"["follow-1","a"]"#).unwrap();
    store::append_event(&home, "follow-2", &OEvent::CardTool { at: "2026-01-01T00:00:04Z".into(), card_id: "a".into(), name: "O 작업 폴더".into(), summary: workspace.to_string_lossy().into_owned() }).unwrap();
    let twice = o_core::run::review_changes(&home, "follow-2", "a").unwrap();
    assert_eq!(twice["baselineRunId"], "run-1");
    assert_eq!(twice["changes"], initial["changes"]);
    store::append_event(&home, "follow-2", &OEvent::CardTool { at: "2026-01-01T00:00:05Z".into(), card_id: "a".into(), name: "O 작업 폴더".into(), summary: project.to_string_lossy().into_owned() }).unwrap();
    assert!(o_core::run::review_changes(&home, "follow-2", "a").is_err());
}

mod support;
