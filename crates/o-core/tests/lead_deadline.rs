use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use o_core::lead::{self, LeadEvent, SendRequest};
use o_core::run::plan::ProviderId;

#[test]
fn closed_stdout_fixture() {
    if std::env::var_os("O_TEST_CLOSED_STDOUT").is_none() { return; }
    #[cfg(windows)]
    unsafe {
        use std::os::windows::io::AsRawHandle;
        windows_sys::Win32::Foundation::CloseHandle(std::io::stdout().as_raw_handle() as _);
    }
    #[cfg(unix)]
    unsafe {
        unsafe extern "C" { fn close(fd: i32) -> i32; }
        close(1);
    }
    std::thread::sleep(Duration::from_secs(8));
}

#[test]
fn closed_stdout_turn() {
    if std::env::var_os("O_TEST_DEADLINE_TURN").is_none() { return; }
    let root = std::env::temp_dir().join(format!("o-lead-deadline-{}", std::process::id()));
    let home = root.join("home");
    let project = root.join("project");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let project = project.to_string_lossy();
    let request = SendRequest { project: &project, provider: ProviderId::Claude, model: None, effort: None, text: "test", attach: &[], clean: None, fresh: false, yolo: false };
    let spawn = |_: &o_core::providers::LaunchSpec, _: &str| {
        let mut command = Command::new(std::env::current_exe()?);
        command.args(["--exact", "closed_stdout_fixture", "--nocapture"]).env("O_TEST_CLOSED_STDOUT", "1");
        Ok(command)
    };
    let events = lead::send_within(&home, &request, &spawn, Duration::from_millis(500)).unwrap();
    assert!(events.iter().any(|event| matches!(event, LeadEvent::Error { text, .. } if text.contains("시간"))), "{events:?}");
}

#[test]
#[ignore = "재현 테스트: stdout EOF 뒤 시간제한이 해제되는 결함이 수정되면 활성화"]
fn deadline_survives_stdout_eof() {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", "closed_stdout_turn", "--nocapture"]).env("O_TEST_DEADLINE_TURN", "1").stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = o_core::sys::proc::Contained::spawn(&mut command).unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if let Some(status) = child.child.try_wait().unwrap() {
            assert!(status.success(), "turn fixture failed: {status}");
            return;
        }
        assert!(Instant::now() < deadline, "turn remained blocked after stdout EOF");
        std::thread::sleep(Duration::from_millis(25));
    }
}
