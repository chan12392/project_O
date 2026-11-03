use std::fs;
use std::process::Command;

#[test]
fn scheduled_request_failure_is_recorded_without_a_real_agent() {
    let root = std::env::temp_dir().join(format!("o-schedule-cli-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let project = root.join("project");
    let user = root.join("user");
    let jobs = root.join("local/project_O/schedules");
    for dir in [&project, &user, &jobs] { fs::create_dir_all(dir).unwrap(); }
    let job = serde_json::json!({ "id": "qa-job", "enabled": true, "name": "qa", "project": project, "provider": "invalid", "text": "test", "start": "2099-12-31T23:30", "repeat": "once", "clean": true, "autoDispatch": false });
    fs::write(jobs.join("qa-job.json"), job.to_string()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_o")).args(["schedule", "run", "qa-job"]).env("LOCALAPPDATA", root.join("local")).env("USERPROFILE", &user).env("HOME", &user).output().unwrap();
    assert!(!output.status.success());
    let last: serde_json::Value = serde_json::from_slice(&fs::read(jobs.join("qa-job.last")).unwrap()).unwrap();
    assert_eq!(last["ok"], false);
    assert!(last["error"].as_str().unwrap().contains("에이전트"));
}
