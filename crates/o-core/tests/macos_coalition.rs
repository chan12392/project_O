#![cfg(target_os = "macos")]
use o_core::sys::{launch::Launch, macos_worker};
use std::io::Read;
#[test]
fn native_worker_round_trip() {
    let mut launch = Launch::new("/usr/bin/printf");
    launch.args(["%s", "quotes ' \" $HOME ; $(false)\n끝"]);
    launch.env_clear();
    let (mut child, tree) = macos_worker::spawn(&launch).unwrap();
    assert!(child.try_wait().unwrap().is_none());
    child.resume().unwrap();
    let mut output = String::new(); child.stdout.take().unwrap().read_to_string(&mut output).unwrap();
    assert!(child.wait().unwrap().success());
    assert_eq!(output, "quotes ' \" $HOME ; $(false)\n끝");
    assert!(tree.lock().unwrap().stop_and_confirm());
}

#[test]
fn exact_environment_and_working_directory() {
    let cwd = std::env::temp_dir().canonicalize().unwrap();
    let mut launch = Launch::new("/usr/bin/env");
    launch.current_dir(&cwd).env_clear().env("ONLY_FIXTURE", "value with spaces");
    let (mut child, tree) = macos_worker::spawn(&launch).unwrap();
    child.resume().unwrap();
    let mut output = String::new(); child.stdout.take().unwrap().read_to_string(&mut output).unwrap();
    assert!(child.wait().unwrap().success()); assert_eq!(output, "ONLY_FIXTURE=value with spaces\n");
    assert!(tree.lock().unwrap().stop_and_confirm());
    let mut launch = Launch::new("/bin/pwd"); launch.current_dir(&cwd);
    let (mut child, _) = macos_worker::spawn(&launch).unwrap(); child.resume().unwrap();
    output.clear(); child.stdout.take().unwrap().read_to_string(&mut output).unwrap();
    assert!(child.wait().unwrap().success()); assert_eq!(output.trim(), cwd.to_str().unwrap());
}

#[test]
fn stopping_one_job_does_not_stop_another_or_neighbor() {
    let mut neighbor = std::process::Command::new("/bin/sleep").arg("20").spawn().unwrap();
    let mut launch = Launch::new("/bin/sleep"); launch.arg("20");
    let (mut first, first_tree) = macos_worker::spawn(&launch).unwrap();
    let (mut second, second_tree) = macos_worker::spawn(&launch).unwrap();
    first.resume().unwrap(); second.resume().unwrap();
    assert!(first_tree.lock().unwrap().stop_and_confirm());
    assert!(first.wait().unwrap().code().is_none());
    assert!(second.try_wait().unwrap().is_none()); assert!(neighbor.try_wait().unwrap().is_none());
    assert!(second_tree.lock().unwrap().stop_and_confirm());
    neighbor.kill().unwrap(); neighbor.wait().unwrap();
}

#[test]
fn stop_before_resume_and_failed_executable() {
    let mut launch = Launch::new("/bin/sleep"); launch.arg("20");
    let (mut child, tree) = macos_worker::spawn(&launch).unwrap();
    assert!(tree.lock().unwrap().stop_and_confirm());
    assert!(!child.wait().unwrap().success());
    assert!(macos_worker::spawn(&Launch::new("/no-such-o-test-program")).is_err());
}

#[test]
fn fake_crashing_owner() {
    let Some(path) = std::env::var_os("O_COALITION_OWNER_TEST") else { return };
    let mut launch = Launch::new("/bin/sleep"); launch.arg("30");
    let (child, _tree) = macos_worker::spawn(&launch).unwrap(); child.resume().unwrap();
    let started = o_core::sys::proc::started_at(child.id()).unwrap();
    std::fs::write(path, format!("{} {started}", child.id())).unwrap();
    std::thread::sleep(std::time::Duration::from_secs(30));
}

#[test]
fn owner_crash_closes_worker_without_stale_pid_signals() {
    let path = std::env::temp_dir().join(format!("o-coalition-crash-{}", std::process::id()));
    let mut owner = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "fake_crashing_owner", "--nocapture"]).env("O_COALITION_OWNER_TEST", &path).spawn().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !path.exists() && std::time::Instant::now() < deadline { std::thread::sleep(std::time::Duration::from_millis(20)); }
    assert!(path.exists());
    let text = std::fs::read_to_string(&path).unwrap(); let mut values = text.split_whitespace();
    let pid = values.next().unwrap().parse().unwrap(); let started = values.next().unwrap().parse().unwrap();
    owner.kill().unwrap(); owner.wait().unwrap();
    while !o_core::sys::proc::definitely_exited(pid, started) && std::time::Instant::now() < deadline { std::thread::sleep(std::time::Duration::from_millis(20)); }
    assert!(o_core::sys::proc::definitely_exited(pid, started));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn full_unread_output_does_not_block_stop_proof() {
    let launch = Launch::new("/usr/bin/yes");
    let (mut child, tree) = macos_worker::spawn(&launch).unwrap(); child.resume().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(150));
    let start = std::time::Instant::now();
    assert!(tree.lock().unwrap().stop_and_confirm());
    assert!(start.elapsed() < std::time::Duration::from_secs(3));
    assert!(!child.wait().unwrap().success());
}

#[test]
#[ignore = "manual installed official CLI smoke"]
fn installed_codex_version() {
    let mut launch = Launch::new("/opt/homebrew/bin/codex"); launch.arg("--version");
    let (mut child, tree) = macos_worker::spawn(&launch).unwrap(); child.resume().unwrap();
    let mut output = String::new(); child.stdout.take().unwrap().read_to_string(&mut output).unwrap();
    eprintln!("output={output:?} status={:?}", child.wait());
    assert!(tree.lock().unwrap().stop_and_confirm());
}
