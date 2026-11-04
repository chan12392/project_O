use std::io::{Read, Write};
use std::process::{Command, Stdio};
use o_core::sys::{launch::Launch, proc};

#[test]
fn launch_fixture() {
    let Some(mode) = std::env::var_os("O_LAUNCH_TEST_MODE") else { return };
    if mode == "controller" {
        for replace in [false, true] {
            let mut launch = Launch::new(std::env::current_exe().unwrap());
            let cwd = std::env::temp_dir();
            launch.args(["--exact", "launch_fixture", "--nocapture"]).current_dir(&cwd);
            if replace { launch.env_clear(); }
            launch.env("O_LAUNCH_TEST_MODE", "child").env("O_LAUNCH_CWD", cwd).env_remove("O_LAUNCH_REMOVE").env("O_LAUNCH_OVERRIDE", "new");
            let mut command = launch.command();
            command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
            let mut child = proc::hide_window(&mut command).spawn().unwrap();
            child.stdin.take().unwrap().write_all(b"input text").unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains("fixture stderr"));
            let stdout = String::from_utf8(output.stdout).unwrap();
            let expected = if replace { "OBSERVED:none:none:new:input text" } else { "OBSERVED:kept:none:new:input text" };
            assert!(stdout.contains(expected), "{stdout}");
        }
    } else {
        assert_eq!(std::env::current_dir().unwrap(), std::path::PathBuf::from(std::env::var_os("O_LAUNCH_CWD").unwrap()));
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input).unwrap();
        let env = |key| std::env::var(key).unwrap_or_else(|_| "none".into());
        println!("OBSERVED:{}:{}:{}:{}", env("O_LAUNCH_KEEP"), env("O_LAUNCH_REMOVE"), env("O_LAUNCH_OVERRIDE"), input);
        eprintln!("fixture stderr");
    }
}

#[test]
fn launch_preserves_environment_arguments_and_pipes() {
    let mut controller = Command::new(std::env::current_exe().unwrap());
    controller.args(["--exact", "launch_fixture", "--nocapture"])
        .env("O_LAUNCH_TEST_MODE", "controller").env("O_LAUNCH_KEEP", "kept")
        .env("O_LAUNCH_REMOVE", "removed").env("O_LAUNCH_OVERRIDE", "old")
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let output = proc::hide_window(&mut controller).output().unwrap();
    assert!(output.status.success(), "{} {}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}
