use std::process::Command;

#[test]
fn check_reports_resource_declarations_and_rejects_unsafe_paths() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("o-cli-resource-{}-{nonce}", std::process::id()));
    let project = base.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let input = base.join("plan.json");
    let mut plan = serde_json::json!({"title":"test","project":project,"cards":[{"id":"a","provider":"claude","task":"test","resources":[{"kind":"project-path","key":"src","mode":"write"}]}]});
    let check = |plan: &serde_json::Value| {
        std::fs::write(&input, serde_json::to_vec(plan).unwrap()).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_o"));
        command.args(["check", input.to_str().unwrap()]).env("LOCALAPPDATA", &base);
        o_core::sys::proc::hide_window(&mut command).output().unwrap()
    };
    let output = check(&plan);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).contains("project-path"));
    plan["cards"][0]["resources"][0]["key"] = "../outside".into();
    assert_eq!(check(&plan).status.code(), Some(2));
}
