#![cfg(windows)]

mod support;
use std::{path::PathBuf, sync::{Arc, Mutex}};
use o_core::run::{events::CardStatus, plan::Plan, runner::{run_plan, Spawner}, store};

fn sample_result() -> String {
    format!("begin-617b{}한글\n\"middle-4f92\"{}end-9c72", "data".repeat(5_000), "data".repeat(5_000))
}

#[test]
fn fake_agent() {
    match std::env::var("O_PROMPT_TEST_ROLE").as_deref() {
        Ok("source") => {
            let result = sample_result();
            println!("{}", serde_json::json!({"type":"result","subtype":"success","is_error":false,"result":result}));
        }
        Ok("target") => {
            let input = std::env::var_os("O_PROMPT_TEST_FILE").unwrap();
            let output = std::env::var_os("O_PROMPT_TEST_OUTPUT").unwrap();
            std::fs::write(output, std::fs::read(input).unwrap()).unwrap();
            println!("done");
        }
        _ => {}
    }
}

#[test]
fn devin_receives_the_complete_large_predecessor_result_without_a_worktree() {
    let base = std::env::temp_dir().join(format!("o-devin-handoff-{}", store::new_run_id(chrono::Utc::now())));
    let project = base.join("project");
    let home = base.join("state");
    std::fs::create_dir_all(&project).unwrap();
    let plan: Plan = serde_json::from_value(serde_json::json!({
        "title":"handoff", "project":project.to_string_lossy(), "cards":[
            {"id":"source","provider":"claude","task":"produce result"},
            {"id":"target","provider":"devin","task":"consume result","depends_on":["source"]}
        ]
    })).unwrap();
    store::create_run(&home, "run", &plan).unwrap();
    let output = base.join("received.txt");
    let observed = Arc::new(Mutex::new(None::<PathBuf>));
    let captured = Arc::clone(&observed);
    let spawner: Arc<Spawner> = Arc::new(move |spec, _| {
        let mut child = support::fake_worker_launch()?;
        child.args(["--exact", "fake_agent", "--nocapture"]);
        if spec.bin == "devin" {
            let at = spec.args.iter().position(|arg| arg == "--prompt-file").expect("long prompt must use a file");
            let path = PathBuf::from(&spec.args[at + 1]);
            assert!(!spec.args.iter().any(|arg| arg.contains("middle-4f92")));
            *captured.lock().unwrap() = Some(path.clone());
            child.env("O_PROMPT_TEST_ROLE", "target").env("O_PROMPT_TEST_FILE", path).env("O_PROMPT_TEST_OUTPUT", &output);
        } else {
            child.env("O_PROMPT_TEST_ROLE", "source");
        }
        Ok(child)
    });
    assert_eq!(run_plan(&home, "run", &plan, None, spawner).unwrap(), CardStatus::Done);
    let received = std::fs::read_to_string(base.join("received.txt")).unwrap();
    assert!(received.contains(&serde_json::to_string(&sample_result()).unwrap()));
    assert!(!observed.lock().unwrap().as_ref().unwrap().exists());
    assert!(!store::run_dir(&home, "run").join("isolated-worktrees").exists());
}
