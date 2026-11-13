//! Direct, unauthenticated exercise of the staged native Windows launcher.
#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
fn main() {
    use std::ffi::OsString;
    use std::io::{Read, Write};
    use o_core::sys::win_launch::Launch;

    let mode = std::env::args_os().nth(1);
    if mode.as_deref() == Some(std::ffi::OsStr::new("--early-exit")) { return; }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--env-probe")) {
        println!("{}", std::env::var_os("O_TEST_REMOVED").is_some());
        return;
    }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--env-audit")) {
        println!("{}", serde_json::json!({"has_key": std::env::var_os("CODEX_API_KEY").is_some(), "removed": std::env::var_os("O_FAKE_REMOVE").is_some(), "home": std::env::var("O_FAKE_HOME").ok(), "inherited": std::env::var("O_FAKE_INHERITED").ok()}));
        return;
    }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--managed-owner")) {
        o_core::sys::win_launch::run_handle_owner(&std::env::args_os().skip(2).collect::<Vec<_>>()).unwrap();
        return;
    }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--handle-owner")) {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::System::Threading::{GetProcessTimes, OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE};
        let pid: u32 = std::env::args().nth(2).unwrap().parse().unwrap();
        let expected: u64 = std::env::args().nth(3).unwrap().parse().unwrap();
        let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if !raw.is_null() {
            let parent = unsafe { OwnedHandle::from_raw_handle(raw) };
            let (mut created, mut exit, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
            if unsafe { GetProcessTimes(parent.as_raw_handle(), &mut created, &mut exit, &mut kernel, &mut user) } != 0 && ((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime)) == expected {
                unsafe { WaitForSingleObject(parent.as_raw_handle(), u32::MAX); }
            }
        }
        return;
    }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--child")) {
        let args: Vec<_> = std::env::args().skip(2).collect();
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input).unwrap();
        println!("{}", serde_json::json!({ "args": args, "input": input, "cwd": std::env::current_dir().unwrap(), "value": std::env::var("O_TEST_VALUE").unwrap(), "missing": std::env::var_os("O_TEST_REMOVED").is_none() }));
        eprint!("stderr-ok");
        return;
    }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--tree-parent")) {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};
        use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--stray").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW).spawn().unwrap();
        println!("O_DESCENDANT_PID={}", child.id());
        std::io::stdout().flush().unwrap();
        let _ = child.wait();
        return;
    }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--owner-before-create")) {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::Threading::GetProcessId;
        let owner = handle_owner();
        let launch = Launch { executable: std::env::current_exe().unwrap(), args: vec!["--early-exit".into()], cwd: std::env::current_dir().unwrap(), environment: vec![] };
        let _ = launch.spawn_with_hook(&owner.0, || {
            println!("O_HANDLE_PID={}", unsafe { GetProcessId(owner.0.as_raw_handle()) });
            std::io::stdout().flush().unwrap();
            std::thread::sleep(std::time::Duration::from_secs(30));
            Ok(())
        });
        return;
    }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--managed-owner-after-create")) {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::Threading::GetProcessId;
        let owner = o_core::sys::win_launch::HandleOwner::start(std::env::current_exe().unwrap(), "--managed-owner").unwrap();
        let launch = Launch { executable: std::env::current_exe().unwrap(), args: vec!["--child".into()], cwd: std::env::current_dir().unwrap(), environment: vec![] };
        let _ = launch.spawn_with_hooks(&owner, || Ok(()), |pid| {
            println!("O_JOB_PID={} O_HANDLE_PID={}", pid, unsafe { GetProcessId(owner.as_raw_handle()) });
            std::io::stdout().flush().unwrap();
            std::thread::sleep(std::time::Duration::from_secs(30));
        });
        return;
    }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--owner")) || mode.as_deref() == Some(std::ffi::OsStr::new("--owner-after-create")) {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::Threading::GetProcessId;
        let owner = handle_owner();
        let launch = Launch { executable: std::env::current_exe().unwrap(), args: vec!["--child".into()], cwd: std::env::current_dir().unwrap(), environment: vec![] };
        if mode.as_deref() == Some(std::ffi::OsStr::new("--owner-after-create")) {
            let _ = launch.spawn_with_hooks(&owner.0, || Ok(()), |pid| {
                println!("O_JOB_PID={} O_HANDLE_PID={}", pid, unsafe { GetProcessId(owner.0.as_raw_handle()) });
                std::io::stdout().flush().unwrap();
                std::thread::sleep(std::time::Duration::from_secs(30));
            });
            return;
        }
        let (child, _job) = launch.spawn(&owner.0).unwrap();
        println!("O_JOB_PID={} O_HANDLE_PID={}", child.id(), unsafe { GetProcessId(owner.0.as_raw_handle()) });
        std::io::stdout().flush().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(30));
        return;
    }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--stray")) {
        println!("ready");
        std::io::stdout().flush().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(10));
        return;
    }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--nested-job-limit")) || mode.as_deref() == Some(std::ffi::OsStr::new("--nested-job-success")) {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::WAIT_TIMEOUT;
        use windows_sys::Win32::System::Threading::WaitForSingleObject;
        let owner = handle_owner();
        let launch = Launch { executable: std::env::current_exe().unwrap(), args: vec!["--early-exit".into()], cwd: std::env::current_dir().unwrap(), environment: vec![] };
        if mode.as_deref() == Some(std::ffi::OsStr::new("--nested-job-limit")) {
            let error = match launch.spawn(&owner.0) { Ok(_) => panic!("outer quota admitted the target"), Err(error) => error };
            println!("nested-limit-refused:{}", error.raw_os_error().unwrap_or(-1));
        } else {
            let (mut child, mut job) = launch.spawn(&owner.0).unwrap();
            assert_token_metadata_matches(&owner.0, child.id());
            assert!(job.stop_and_confirm(), "nested target job did not reach zero");
            child.wait().unwrap();
            assert_eq!(unsafe { WaitForSingleObject(owner.0.as_raw_handle(), 0) }, WAIT_TIMEOUT);
            println!("nested-target-stopped-owner-alive");
        }
        return;
    }
    // This test binary is single-threaded here; the real user environment is never changed.
    unsafe { std::env::set_var("O_TEST_REMOVED", "not-a-secret") };
    assert_command_cannot_expose_environment_clearing();
    let root = std::env::temp_dir();
    let owner = handle_owner();
    let arguments = ["", "한글 공백", "quote\"inside", "trailing\\", "\\\"", "tab\tvalue"];
    let mut environment = vec![(OsString::from("O_TEST_VALUE"), OsString::from("값 공백"))];
    if let Some(root) = std::env::var_os("SystemRoot") { environment.push((OsString::from("SystemRoot"), root)); }
    let launch = Launch { executable: std::env::current_exe().unwrap(), args: std::iter::once(OsString::from("--child")).chain(arguments.iter().map(OsString::from)).collect(), cwd: root.clone(), environment };
    let (mut child, mut job) = launch.spawn(&owner.0).unwrap();
    assert_token_metadata_matches(&owner.0, child.id());
    assert!(child.try_wait().unwrap().is_none());
    child.resume().unwrap();
    let mut input = child.stdin.take().unwrap();
    input.write_all("입력".as_bytes()).unwrap();
    drop(input);
    let mut stdout = String::new();
    child.stdout.take().unwrap().read_to_string(&mut stdout).unwrap();
    let mut stderr = String::new();
    child.stderr.take().unwrap().read_to_string(&mut stderr).unwrap();
    assert!(child.wait().unwrap().success());
    assert!(job.stop_and_confirm());
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(value["args"], serde_json::json!(arguments));
    assert_eq!(value["input"], "입력");
    assert_eq!(value["value"], "값 공백");
    assert_eq!(value["missing"], true);
    assert_eq!(std::path::Path::new(value["cwd"].as_str().unwrap()), root);
    assert_eq!(stderr, "stderr-ok");
    let (mut suspended, job) = launch.spawn(&owner.0).unwrap();
    assert!(suspended.try_wait().unwrap().is_none());
    drop(job);
    suspended.wait().unwrap();
    assert_inner_job_excludes_handle_owner(&launch, &owner.0);
    assert_descendant_stops_with_job(&launch, &owner.0, false);
    assert_descendant_stops_with_job(&launch, &owner.0, true);
    assert_nested_job_limit_fails_closed(false);
    assert_nested_job_limit_fails_closed(true);
    owner_crash(false, "--owner");
    owner_crash(true, "--owner");
    owner_crash(false, "--owner-after-create");
    owner_crash(true, "--owner-after-create");
    owner_crash(false, "--managed-owner-after-create");
    owner_crash(true, "--managed-owner-after-create");
    owner_crash_before_creation();
    demonstrate_original_cross_spawn_handle_leak();
    assert_native_launch_isolated_from_parallel_spawn(&launch, &owner.0);
    assert_parallel_native_launches(&launch, &owner.0);
    assert_failures_do_not_leak_remote_handles(&launch, &owner.0);
    assert_dead_handle_owner_fails_closed(&launch);
    assert_owner_rejects_wrong_parent_identity();
    assert_managed_owner_handshake_and_lifecycle(&launch);
    assert_explicit_environment_in_real_child(&launch, &owner.0);
    assert_common_native_child(&launch, &owner.0);
    // cmd's hidden variables start with `=` and are valid names; other `=` placements are not.
    let names = |names: &[&str]| Launch { executable: launch.executable.clone(), args: vec![], cwd: launch.cwd.clone(), environment: names.iter().map(|name| (OsString::from(name), OsString::from("x"))).collect() }.environment_block();
    assert!(names(&["=C:", "=ExitCode", "=::"]).is_ok());
    for bad in ["=", "A=B", "=A=B", ""] { assert!(names(&[bad]).is_err(), "{bad:?}"); }
    let mut duplicate = launch;
    duplicate.environment.push((OsString::from("o_test_value"), OsString::from("duplicate")));
    assert!(duplicate.spawn(&owner.0).is_err());
    duplicate.executable = "relative.exe".into();
    assert!(duplicate.spawn(&owner.0).is_err());
    println!("native Windows process checks passed");
}

#[cfg(windows)]
fn assert_common_native_child(launch: &o_core::sys::win_launch::Launch, owner: &std::os::windows::io::OwnedHandle) {
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};
    let (native, tree) = launch.spawn(owner).unwrap();
    let tree = Some(Arc::new(Mutex::new(tree)));
    let mut child = o_core::sys::worker::Child::native(native);
    assert!(child.try_wait(&tree).unwrap().is_none());
    child.resume().unwrap();
    child.stdin.take().unwrap().write_all(b"common-child").unwrap();
    let mut stdout = String::new();
    child.stdout.take().unwrap().read_to_string(&mut stdout).unwrap();
    let mut stderr = String::new();
    child.stderr.take().unwrap().read_to_string(&mut stderr).unwrap();
    assert!(child.wait(&tree).unwrap().success());
    assert_eq!(serde_json::from_str::<serde_json::Value>(stdout.trim()).unwrap()["input"], "common-child");
    assert_eq!(stderr, "stderr-ok");
}

#[cfg(windows)]
fn assert_explicit_environment_in_real_child(launch: &o_core::sys::win_launch::Launch, owner: &std::os::windows::io::OwnedHandle) {
    use std::io::Read;
    use o_core::sys::launch::{EnvironmentBase, Launch};
    let mut inherited = vec![("codex_api_key".into(), "fake-only".into()), ("O_FAKE_INHERITED".into(), "kept".into()), ("O_FAKE_HOME".into(), "own".into()), ("o_fake_remove".into(), "removed".into())];
    if let Some(root) = std::env::var_os("SystemRoot") { inherited.push(("SystemRoot".into(), root)); }
    for base in [EnvironmentBase::Inherit, EnvironmentBase::Replace] {
        let mut changes = vec![("O_FAKE_REMOVE".into(), None)];
        if base == EnvironmentBase::Replace {
            changes.push(("O_FAKE_HOME".into(), Some("clean".into())));
            if let Some(root) = std::env::var_os("SystemRoot") { changes.push(("SystemRoot".into(), Some(root))); }
        }
        let spec = Launch { executable: launch.executable.clone(), args: vec!["--env-audit".into()], cwd: launch.cwd.clone(), environment_base: base, environment_changes: changes, handle_owner_executable: None };
        let native = spec.native(inherited.clone()).unwrap();
        let (mut child, _job) = native.spawn(owner).unwrap();
        child.resume().unwrap();
        let mut output = String::new();
        child.stdout.take().unwrap().read_to_string(&mut output).unwrap();
        assert!(child.wait().unwrap().success());
        let value: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        assert_eq!(value["has_key"], base == EnvironmentBase::Inherit);
        assert_eq!(value["removed"], false);
        assert_eq!(value["home"], if base == EnvironmentBase::Inherit { "own" } else { "clean" });
        if base == EnvironmentBase::Inherit { assert_eq!(value["inherited"], "kept"); } else { assert!(value["inherited"].is_null()); }
    }
}

#[cfg(windows)]
fn assert_command_cannot_expose_environment_clearing() {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
    let mut inherited = Command::new(std::env::current_exe().unwrap());
    let mut cleared = Command::new(std::env::current_exe().unwrap());
    cleared.env_clear();
    for command in [&mut inherited, &mut cleared] {
        command.arg("--env-probe").creation_flags(CREATE_NO_WINDOW);
        if let Some(root) = std::env::var_os("SystemRoot") { command.env("SystemRoot", root); }
    }
    assert_eq!(inherited.get_envs().collect::<Vec<_>>(), cleared.get_envs().collect::<Vec<_>>());
    assert_eq!(inherited.output().unwrap().stdout, b"true\n");
    assert_eq!(cleared.output().unwrap().stdout, b"false\n");
}

#[cfg(windows)]
fn assert_descendant_stops_with_job(launch: &o_core::sys::win_launch::Launch, owner: &std::os::windows::io::OwnedHandle, close_job: bool) {
    use std::io::{BufRead, BufReader};
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    let spec = o_core::sys::win_launch::Launch { executable: launch.executable.clone(), args: vec!["--tree-parent".into()], cwd: launch.cwd.clone(), environment: launch.environment.clone() };
    let (mut child, mut job) = spec.spawn(owner).unwrap();
    child.resume().unwrap();
    let output = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || { let mut line = String::new(); let _ = BufReader::new(output).read_line(&mut line); let _ = tx.send(line); });
    let line = rx.recv_timeout(std::time::Duration::from_secs(10)).expect("descendant was not started");
    let pid: u32 = line.trim().strip_prefix("O_DESCENDANT_PID=").unwrap().parse().unwrap();
    let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    assert!(!raw.is_null());
    let descendant = unsafe { OwnedHandle::from_raw_handle(raw) };
    if close_job { drop(job); } else { assert!(job.stop_and_confirm()); }
    assert_eq!(unsafe { WaitForSingleObject(descendant.as_raw_handle(), 10_000) }, WAIT_OBJECT_0);
    child.wait().unwrap();
}

#[cfg(windows)]
fn assert_managed_owner_handshake_and_lifecycle(launch: &o_core::sys::win_launch::Launch) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    let executable = std::env::current_exe().unwrap();
    assert!(o_core::sys::win_launch::HandleOwner::start(executable.clone(), "--early-exit").is_err());
    let owner = o_core::sys::win_launch::HandleOwner::start(executable, "--managed-owner").unwrap();
    let pid = unsafe { windows_sys::Win32::System::Threading::GetProcessId(owner.as_raw_handle()) };
    let helper_raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    assert!(!helper_raw.is_null());
    let (mut child, mut job) = launch.spawn(&owner).unwrap();
    assert_token_metadata_matches(&owner, child.id());
    assert!(job.stop_and_confirm());
    child.wait().unwrap();
    drop(owner);
    assert_eq!(unsafe { WaitForSingleObject(helper_raw, 5000) }, WAIT_OBJECT_0);
    unsafe { windows_sys::Win32::Foundation::CloseHandle(helper_raw); }
}

#[cfg(windows)]
fn assert_token_metadata_matches(owner: &impl std::os::windows::io::AsRawHandle, child_pid: u32) {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Security::{GetTokenInformation, TokenStatistics, TOKEN_QUERY, TOKEN_STATISTICS};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION};
    let stats = |process| {
        let mut raw = std::ptr::null_mut();
        assert_ne!(unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut raw) }, 0);
        let token = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut info = TOKEN_STATISTICS::default();
        let mut len = 0;
        assert_ne!(unsafe { GetTokenInformation(token.as_raw_handle(), TokenStatistics, (&mut info as *mut TOKEN_STATISTICS).cast(), std::mem::size_of_val(&info) as u32, &mut len) }, 0);
        (info.AuthenticationId.HighPart, info.AuthenticationId.LowPart, info.TokenType, info.GroupCount, info.PrivilegeCount)
    };
    let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, child_pid) };
    assert!(!raw.is_null());
    let target = unsafe { OwnedHandle::from_raw_handle(raw) };
    let current = stats(unsafe { GetCurrentProcess() });
    assert!(current == stats(owner.as_raw_handle()), "helper token metadata differs");
    assert!(current == stats(target.as_raw_handle()), "target token metadata differs");
}

#[cfg(windows)]
fn assert_inner_job_excludes_handle_owner(launch: &o_core::sys::win_launch::Launch, owner: &std::os::windows::io::OwnedHandle) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::WAIT_TIMEOUT;
    use windows_sys::Win32::System::Threading::WaitForSingleObject;
    let (mut child, mut job) = launch.spawn(owner).unwrap();
    assert!(job.stop_and_confirm(), "the target job must reach zero active processes");
    child.wait().unwrap();
    assert_eq!(unsafe { WaitForSingleObject(owner.as_raw_handle(), 0) }, WAIT_TIMEOUT, "stopping the target job also stopped the handle owner");
}

#[cfg(windows)]
fn assert_nested_job_limit_fails_closed(limited: bool) {
    use std::io::{BufRead, BufReader};
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use windows_sys::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE};
    use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};
    let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    assert!(!raw.is_null());
    let outer = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_ACTIVE_PROCESS | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    limits.BasicLimitInformation.ActiveProcessLimit = if limited { 2 } else { 3 };
    assert_ne!(unsafe { SetInformationJobObject(raw, JobObjectExtendedLimitInformation, (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(), std::mem::size_of_val(&limits) as u32) }, 0);
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.arg(if limited { "--nested-job-limit" } else { "--nested-job-success" }).env_clear().stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
    if let Some(root) = std::env::var_os("SystemRoot") { command.env("SystemRoot", root); }
    let mut child = command.spawn().unwrap();
    assert_ne!(unsafe { AssignProcessToJobObject(outer.as_raw_handle(), child.as_raw_handle()) }, 0);
    o_core::sys::proc::resume_worker(child.id()).unwrap();
    let output = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || { let mut line = String::new(); let _ = BufReader::new(output).read_line(&mut line); let _ = tx.send(line); });
    let line = rx.recv_timeout(std::time::Duration::from_secs(10)).expect("nested quota check timed out");
    if limited {
        assert_eq!(line.trim(), "nested-limit-refused:1816", "unexpected outer job error");
        println!("{}", line.trim());
    } else {
        assert_eq!(line.trim(), "nested-target-stopped-owner-alive");
    }
    assert!(child.wait().unwrap().success());
    drop(outer);
}

#[cfg(windows)]
fn owner_crash_before_creation() {
    use std::io::{BufRead, BufReader};
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, CREATE_NO_WINDOW, PROCESS_SYNCHRONIZE};
    struct Owner(std::process::Child);
    impl Drop for Owner { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.arg("--owner-before-create").env_clear().stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).creation_flags(CREATE_NO_WINDOW);
    if let Some(root) = std::env::var_os("SystemRoot") { command.env("SystemRoot", root); }
    let mut owner = Owner(command.spawn().unwrap());
    let output = owner.0.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || { let mut line = String::new(); let _ = BufReader::new(output).read_line(&mut line); let _ = tx.send(line); });
    let line = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
    let helper_pid: u32 = line.trim().strip_prefix("O_HANDLE_PID=").unwrap().parse().unwrap();
    let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, helper_pid) };
    assert!(!raw.is_null());
    let helper = unsafe { OwnedHandle::from_raw_handle(raw) };
    owner.0.kill().unwrap();
    owner.0.wait().unwrap();
    assert_eq!(unsafe { WaitForSingleObject(helper.as_raw_handle(), 10_000) }, WAIT_OBJECT_0);
}

#[cfg(windows)]
fn assert_parallel_native_launches(launch: &o_core::sys::win_launch::Launch, owner: &std::os::windows::io::OwnedHandle) {
    use std::io::{Read, Write};
    use std::sync::{Arc, Barrier};
    let barrier = Arc::new(Barrier::new(8));
    std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for id in 0..8 {
            let barrier = barrier.clone();
            workers.push(scope.spawn(move || {
                let spec = o_core::sys::win_launch::Launch { executable: launch.executable.clone(), args: vec!["--child".into(), id.to_string().into()], cwd: launch.cwd.clone(), environment: launch.environment.clone() };
                let (mut child, _job) = spec.spawn_with_hook(owner, || { barrier.wait(); Ok(()) }).unwrap();
                child.resume().unwrap();
                child.stdin.take().unwrap().write_all(id.to_string().as_bytes()).unwrap();
                let mut stdout = String::new();
                child.stdout.take().unwrap().read_to_string(&mut stdout).unwrap();
                assert!(child.wait().unwrap().success());
                let value: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
                assert_eq!(value["args"], serde_json::json!([id.to_string()]));
                assert_eq!(value["input"], id.to_string());
            }));
        }
        for worker in workers { worker.join().unwrap(); }
    });
}

#[cfg(windows)]
fn assert_failures_do_not_leak_remote_handles(launch: &o_core::sys::win_launch::Launch, owner: &std::os::windows::io::OwnedHandle) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Threading::GetProcessHandleCount;
    let count = || {
        let mut count = 0;
        assert_ne!(unsafe { GetProcessHandleCount(owner.as_raw_handle(), &mut count) }, 0);
        count
    };
    let before = count();
    for _ in 0..8 {
        assert!(launch.spawn_with_hook(owner, || Err(std::io::Error::other("injected before creation"))).is_err());
    }
    assert!(count() <= before + 1, "failed launches leaked handles in the owner");
    let panic = std::panic::catch_unwind(|| {
        let _ = launch.spawn_with_hook(owner, || -> std::io::Result<()> { panic!("injected before creation") });
    });
    assert!(panic.is_err());
    assert!(count() <= before + 1, "panic leaked handles in the owner");
    let invalid = o_core::sys::win_launch::Launch { executable: launch.cwd.join("missing-native-test.exe"), args: vec![], cwd: launch.cwd.clone(), environment: launch.environment.clone() };
    assert!(invalid.spawn(owner).is_err());
    assert!(count() <= before + 1, "CreateProcess failure leaked handles in the owner");
}

#[cfg(windows)]
fn assert_dead_handle_owner_fails_closed(launch: &o_core::sys::win_launch::Launch) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Threading::{TerminateProcess, WaitForSingleObject};
    let owner = handle_owner();
    let result = launch.spawn_with_hook(&owner.0, || {
        assert_ne!(unsafe { TerminateProcess(owner.0.as_raw_handle(), 1) }, 0);
        unsafe { WaitForSingleObject(owner.0.as_raw_handle(), 5000); }
        Ok(())
    });
    assert!(result.is_err(), "a dead handle owner must not produce a runnable child");
}

#[cfg(windows)]
fn demonstrate_original_cross_spawn_handle_leak() {
    use std::io::{BufRead, BufReader, Read};
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
    let mut read = std::ptr::null_mut();
    let mut write = std::ptr::null_mut();
    let attributes = SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: std::ptr::null_mut(), bInheritHandle: 1 };
    assert_ne!(unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) }, 0);
    let read = unsafe { OwnedHandle::from_raw_handle(read) };
    let write = unsafe { OwnedHandle::from_raw_handle(write) };
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.arg("--stray").env_clear().stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).creation_flags(CREATE_NO_WINDOW);
    if let Some(root) = std::env::var_os("SystemRoot") { command.env("SystemRoot", root); }
    let mut child = command.spawn().unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "ready");
    drop(write);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut file = std::fs::File::from(read);
        let mut byte = [0u8; 1];
        let _ = tx.send(file.read(&mut byte));
    });
    let eof_before_unrelated_process_exits = rx.recv_timeout(std::time::Duration::from_millis(300)).is_ok();
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(!eof_before_unrelated_process_exits, "the original inheritance-race control did not reproduce");
}

#[cfg(windows)]
struct HandleOwner(std::os::windows::io::OwnedHandle);

#[cfg(windows)]
impl Drop for HandleOwner {
    fn drop(&mut self) {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::Threading::{TerminateProcess, WaitForSingleObject};
        unsafe { TerminateProcess(self.0.as_raw_handle(), 1); WaitForSingleObject(self.0.as_raw_handle(), 5000); }
    }
}

#[cfg(windows)]
fn handle_owner() -> HandleOwner {
    handle_owner_with_identity(o_core::sys::proc::started_at(std::process::id()).unwrap())
}

#[cfg(windows)]
fn handle_owner_with_identity(created: u64) -> HandleOwner {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{CreateProcessW, CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION, STARTUPINFOW};
    let executable = std::env::current_exe().unwrap();
    let app: Vec<u16> = executable.as_os_str().encode_wide().chain([0]).collect();
    let mut command: Vec<u16> = format!("\"{}\" --handle-owner {} {}", executable.display(), std::process::id(), created).encode_utf16().chain([0]).collect();
    let mut environment: Vec<u16> = std::env::var_os("SystemRoot").map(|root| {
        let mut v: Vec<u16> = "SystemRoot=".encode_utf16().collect();
        v.extend(root.encode_wide()); v.push(0); v
    }).unwrap_or_else(|| vec![0]);
    environment.push(0);
    let startup = STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, ..Default::default() };
    let mut info = PROCESS_INFORMATION::default();
    assert_ne!(unsafe { CreateProcessW(app.as_ptr(), command.as_mut_ptr(), std::ptr::null(), std::ptr::null(), 0, CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT, environment.as_ptr().cast(), std::ptr::null(), &startup, &mut info) }, 0, "{}", std::io::Error::last_os_error());
    unsafe { CloseHandle(info.hThread) };
    HandleOwner(unsafe { OwnedHandle::from_raw_handle(info.hProcess) })
}

#[cfg(windows)]
fn assert_owner_rejects_wrong_parent_identity() {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::WaitForSingleObject;
    let expected = o_core::sys::proc::started_at(std::process::id()).unwrap();
    let owner = handle_owner_with_identity(expected.wrapping_add(1));
    assert_eq!(unsafe { WaitForSingleObject(owner.0.as_raw_handle(), 5000) }, WAIT_OBJECT_0);
}

#[cfg(windows)]
fn assert_native_launch_isolated_from_parallel_spawn(launch: &o_core::sys::win_launch::Launch, owner: &std::os::windows::io::OwnedHandle) {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
    struct Stray(std::process::Child);
    impl Drop for Stray { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }
    let mut stray = None;
    let quick = o_core::sys::win_launch::Launch { executable: launch.executable.clone(), args: vec!["--early-exit".into()], cwd: launch.cwd.clone(), environment: launch.environment.clone() };
    let (mut child, _job) = quick.spawn_with_hook(owner, || {
        let mut command = Command::new(std::env::current_exe()?);
        command.arg("--stray").env_clear().stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).creation_flags(CREATE_NO_WINDOW);
        if let Some(root) = std::env::var_os("SystemRoot") { command.env("SystemRoot", root); }
        let mut process = command.spawn()?;
        let mut line = String::new();
        BufReader::new(process.stdout.take().unwrap()).read_line(&mut line)?;
        assert_eq!(line.trim(), "ready");
        stray = Some(Stray(process));
        Ok(())
    }).unwrap();
    child.resume().unwrap();
    let mut input = child.stdin.take().unwrap();
    assert!(child.wait().unwrap().success());
    assert!(input.write_all(b"probe").is_err(), "unrelated process retained target stdin");
    let output = child.stdout.take().unwrap();
    let error = child.stderr.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let err_tx = tx.clone();
    std::thread::spawn(move || { let mut text = String::new(); let _ = tx.send(BufReader::new(output).read_to_string(&mut text)); });
    std::thread::spawn(move || { let mut text = String::new(); let _ = err_tx.send(BufReader::new(error).read_to_string(&mut text)); });
    for _ in 0..2 { assert!(rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap().is_ok(), "unrelated process retained a target output pipe"); }
    drop(stray);
}

#[cfg(windows)]
fn owner_crash(nested: bool, mode: &str) {
    use std::io::{BufRead, BufReader};
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED, PROCESS_SYNCHRONIZE};

    struct Owner(std::process::Child);
    impl Drop for Owner { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.arg(mode).env_clear().stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW | if nested { CREATE_SUSPENDED } else { 0 });
    if let Some(root) = std::env::var_os("SystemRoot") { command.env("SystemRoot", root); }
    let mut owner = Owner(command.spawn().unwrap());
    let _outer = if nested {
        let job = o_core::sys::proc::contain(&owner.0).unwrap();
        o_core::sys::proc::resume_worker(owner.0.id()).unwrap();
        Some(job)
    } else { None };
    let output = owner.0.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(output).read_line(&mut line);
        let _ = tx.send(line);
    });
    let line = rx.recv_timeout(std::time::Duration::from_secs(10)).expect("owner did not report child");
    let mut parts = line.split_whitespace();
    let pid: u32 = parts.next().unwrap().strip_prefix("O_JOB_PID=").unwrap().parse().unwrap();
    let helper_pid: u32 = parts.next().unwrap().strip_prefix("O_HANDLE_PID=").unwrap().parse().unwrap();
    let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    let helper_raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, helper_pid) };
    assert!(!raw.is_null() && !helper_raw.is_null());
    let child = unsafe { OwnedHandle::from_raw_handle(raw) };
    let helper = unsafe { OwnedHandle::from_raw_handle(helper_raw) };
    owner.0.kill().unwrap();
    owner.0.wait().unwrap();
    assert_eq!(unsafe { WaitForSingleObject(child.as_raw_handle(), 10_000) }, WAIT_OBJECT_0);
    assert_eq!(unsafe { WaitForSingleObject(helper.as_raw_handle(), 10_000) }, WAIT_OBJECT_0);
}
