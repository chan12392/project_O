use std::process::Command;

#[test]
fn doctor_warns_about_auth_names_without_printing_values() {
    let root = std::env::temp_dir().join(format!("o-doctor-auth-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_o"));
    command.args(["agents", "doctor"]).env("PATH", &root).env("LOCALAPPDATA", &root)
        .env("ANTHROPIC_API_KEY", "test-only-secret");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command.output().unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(output.status.success(), "{text}");
    assert!(text.contains("ANTHROPIC_API_KEY") && text.contains("구독 대신 API로 과금"));
    assert!(!text.contains("test-only-secret"));
}
