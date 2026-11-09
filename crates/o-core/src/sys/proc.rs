//! Process plumbing: hidden windows, detached runners, kill-on-close job
//! objects, short captured commands, liveness checks and tree kills. Windows
//! first; elsewhere the nearest equivalent.

use std::collections::BTreeMap;
#[cfg(windows)]
use std::ffi::OsStr;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(windows)]
mod win {
    pub use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, DETACHED_PROCESS};
}

/// No console window for a child process.
pub fn hide_window(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(win::CREATE_NO_WINDOW);
    }
    command
}

/// A worker must not create descendants before its job object contains it.
pub fn spawn_worker(command: &mut Command) -> io::Result<Child> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;
        command.creation_flags(win::CREATE_NO_WINDOW | CREATE_SUSPENDED);
    }
    command.spawn()
}

#[cfg(windows)]
pub fn resume_worker(pid: u32) -> io::Result<()> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Thread32First, Thread32Next, THREADENTRY32, TH32CS_SNAPTHREAD};
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE { return Err(io::Error::last_os_error()); }
        let mut entry: THREADENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut found = false;
        let mut ok = Thread32First(snapshot, &mut entry);
        while ok != 0 {
            if entry.th32OwnerProcessID == pid {
                let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                if thread.is_null() {
                    let error = io::Error::last_os_error();
                    CloseHandle(snapshot);
                    return Err(error);
                }
                let result = ResumeThread(thread);
                let error = (result == u32::MAX).then(io::Error::last_os_error);
                CloseHandle(thread);
                if result != 1 {
                    CloseHandle(snapshot);
                    return Err(error.unwrap_or_else(|| io::Error::other("프로세스 시작 상태를 확인하지 못했습니다")));
                }
                found = true;
            }
            ok = Thread32Next(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
        if found { Ok(()) } else { Err(io::Error::other("시작할 작업 프로세스 스레드를 찾지 못했습니다")) }
    }
}

#[cfg(not(windows))]
pub fn resume_worker(_pid: u32) -> io::Result<()> { Ok(()) }

/// Starts a process that outlives this one, with no console and no stdio.
pub fn spawn_detached(command: &mut Command) -> std::io::Result<Child> {
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(win::DETACHED_PROCESS | win::CREATE_NEW_PROCESS_GROUP | win::CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command.spawn()
}

/// Windows holds a kill-on-close job; Linux only tracks the directly owned child.
pub struct KillOnClose {
    #[cfg(target_os = "macos")]
    pub(crate) monitor: super::macos_worker::Monitor,
    #[cfg(windows)]
    pub(crate) job: windows_sys::Win32::Foundation::HANDLE,
    #[cfg(target_os = "linux")]
    pid: u32,
    #[cfg(target_os = "linux")]
    reaped: bool,
}

// The job handle is only closed once, by Drop.
unsafe impl Send for KillOnClose {}

impl Drop for KillOnClose {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.job);
        }
    }
}

impl KillOnClose {
    /// Stop the job and confirm that no processes remain before reusing resources.
    #[cfg(windows)]
    pub fn stop_and_confirm(&mut self) -> bool {
        use windows_sys::Win32::System::JobObjects::{JobObjectBasicAccountingInformation, QueryInformationJobObject, TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION};
        unsafe {
            let _ = TerminateJobObject(self.job, 1);
            for _ in 0..100 {
                let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = std::mem::zeroed();
                if QueryInformationJobObject(self.job, JobObjectBasicAccountingInformation, (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(), std::mem::size_of_val(&info) as u32, std::ptr::null_mut()) == 0 { return false; }
                if info.ActiveProcesses == 0 { return true; }
                thread::sleep(Duration::from_millis(50));
            }
            false
        }
    }

    #[cfg(target_os = "linux")]
    pub fn stop_and_confirm(&mut self) -> bool {
        unsafe extern "C" { fn kill(pid: i32, signal: i32) -> i32; }
        if !self.reaped { unsafe { kill(self.pid as i32, 9); } }
        // shortcut: a stopped parent does not prove descendant exit, retain the reservation.
        false
    }

    #[cfg(target_os = "macos")]
    pub fn stop_and_confirm(&mut self) -> bool { self.monitor.stop_and_confirm() }

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    pub fn stop_and_confirm(&mut self) -> bool { false }
}

/// On Windows, puts a freshly spawned child in a job object that kills it, and every
/// process it started, when the job handle closes. `None` when the system refuses.
#[cfg(windows)]
pub fn contain(child: &Child) -> Option<KillOnClose> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return None;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let set = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if set == 0 || AssignProcessToJobObject(job, child.as_raw_handle() as _) == 0 {
            CloseHandle(job);
            return None;
        }
        Some(KillOnClose { job })
    }
}

#[cfg(target_os = "linux")]
pub fn contain(child: &Child) -> Option<KillOnClose> {
    Some(KillOnClose { pid: child.id(), reaped: false })
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn contain(_child: &Child) -> Option<KillOnClose> {
    None
}

/// Serialize reaping with an owned stop, so a reused PID is never signalled.
pub fn try_wait_worker(child: &mut Child, tree: &Option<Arc<Mutex<KillOnClose>>>) -> io::Result<Option<ExitStatus>> {
    #[cfg(target_os = "linux")]
    if let Some(tree) = tree {
        let mut tree = tree.lock().map_err(|_| io::Error::other("프로세스 상태 잠금 실패"))?;
        let status = child.try_wait()?;
        if status.is_some() { tree.reaped = true; }
        return Ok(status);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = tree;
    child.try_wait()
}

pub fn wait_worker(child: &mut Child, tree: &Option<Arc<Mutex<KillOnClose>>>) -> io::Result<ExitStatus> {
    #[cfg(target_os = "linux")]
    loop {
        if let Some(status) = try_wait_worker(child, tree)? { return Ok(status); }
        thread::sleep(Duration::from_millis(25));
    }
    #[cfg(not(target_os = "linux"))]
    { let _ = tree; child.wait() }
}

/// A short-lived child with no window whose process tree is stopped however
/// the caller lets go of it (usage probes, status checks, installs).
pub struct Contained {
    pub child: Child,
    _tree: Option<KillOnClose>,
}

impl Contained {
    pub fn spawn(command: &mut Command) -> io::Result<Contained> {
        let child = hide_window(command).spawn()?;
        let tree = contain(&child);
        Ok(Contained { child, _tree: tree })
    }
}

impl Drop for Contained {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Runs a short, non-interactive command with no window and a deadline, and
/// returns its exit status, stdout and stderr. A command still running at the
/// deadline is ended with its tree.
pub fn capture(command: &mut Command, timeout: Duration) -> Result<(ExitStatus, String, String), String> {
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut probe = Contained::spawn(command).map_err(|e| format!("실행 실패: {e}"))?;
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        thread::spawn(move || {
            let mut out = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut out);
            }
            String::from_utf8_lossy(&out).into_owned()
        })
    };
    let stdout = drain(probe.child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let stderr = drain(probe.child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));

    let deadline = Instant::now() + timeout;
    let status = loop {
        match probe.child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Ok(None) => return Err("실패 (시간 초과)".into()),
            Err(e) => return Err(format!("실행 실패: {e}")),
        }
    };
    // Ends anything the command left behind holding the pipes open.
    drop(probe);
    Ok((status, stdout.join().unwrap_or_default(), stderr.join().unwrap_or_default()))
}

/// Whether a process with this id is running (one we may not open still counts).
#[cfg(windows)]
pub fn is_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ACCESS_DENIED, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return GetLastError() == ERROR_ACCESS_DENIED;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(handle, &mut code);
        CloseHandle(handle);
        ok != 0 && code == STILL_ACTIVE as u32
    }
}

/// Only a missing process, an exited process, or a different creation time proves this owner is gone.
#[cfg(windows)]
pub fn definitely_exited(pid: u32, expected: u64) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_INVALID_PARAMETER, FILETIME, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    if pid == 0 { return false; }
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() { return GetLastError() == ERROR_INVALID_PARAMETER; }
        let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
        let times_ok = GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) != 0;
        let mut code = 0u32;
        let exit_ok = GetExitCodeProcess(handle, &mut code) != 0;
        CloseHandle(handle);
        if !times_ok || !exit_ok { return false; }
        let actual = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
        actual != expected || code != STILL_ACTIVE as u32
    }
}

#[cfg(target_os = "linux")]
pub fn definitely_exited(pid: u32, expected: u64) -> bool {
    // A missing proc entry alone is inconclusive when procfs hides other processes.
    if pid == 0 || pid > i32::MAX as u32 || !proc_self_matches() { return false; }
    if let Some(actual) = started_at(pid) { return actual != expected; }
    unsafe extern "C" { fn kill(pid: i32, signal: i32) -> i32; }
    const ESRCH: i32 = 3;
    unsafe { kill(pid as i32, 0) == -1 && io::Error::last_os_error().raw_os_error() == Some(ESRCH) }
}

#[cfg(target_os = "macos")]
fn mac_process_start(pid: u32) -> io::Result<u64> {
    let pid = i32::try_from(pid).ok().filter(|pid| *pid > 0).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid pid"))?;
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>();
    unsafe { *libc::__error() = 0; }
    let read = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&mut info as *mut libc::proc_bsdinfo).cast(), size as i32) };
    if read == 0 { return Err(io::Error::last_os_error()); }
    if read != size as i32 || info.pbi_pid != pid as u32 || info.pbi_start_tvusec >= 1_000_000 {
        return Err(io::Error::other("invalid process identity"));
    }
    info.pbi_start_tvsec.checked_mul(1_000_000).and_then(|s| s.checked_add(info.pbi_start_tvusec))
        .ok_or_else(|| io::Error::other("invalid process start time"))
}

#[cfg(target_os = "macos")]
pub fn definitely_exited(pid: u32, expected: u64) -> bool {
    let Ok(native_pid) = i32::try_from(pid) else { return false };
    if native_pid <= 0 { return false; }
    match mac_process_start(pid) {
        Ok(actual) => actual != expected,
        Err(error) if error.raw_os_error() == Some(libc::ESRCH) => {
            unsafe { libc::kill(native_pid, 0) == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) }
        }
        Err(_) => false,
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub fn definitely_exited(_pid: u32, _expected: u64) -> bool { false }

#[cfg(not(windows))]
pub fn is_alive(pid: u32) -> bool {
    // `kill` reads a pid above i32::MAX as negative (-1 signals every process), and 0 is our own group.
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    Command::new("kill").args(["-0", &pid.to_string()]).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// When a running process started, in the system's own units (Windows: FILETIME; Linux:
/// clock ticks since boot). With its pid it names one process: a pid the system handed to
/// another process since has another start time. `None` when it cannot be read.
#[cfg(windows)]
pub fn started_at(pid: u32) -> Option<u64> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
        let ok = GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user);
        CloseHandle(handle);
        (ok != 0).then(|| (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }
}

#[cfg(target_os = "macos")]
pub fn started_at(pid: u32) -> Option<u64> { mac_process_start(pid).ok() }

#[cfg(not(any(windows, target_os = "macos")))]
pub fn started_at(pid: u32) -> Option<u64> {
    #[cfg(target_os = "linux")]
    if !proc_self_matches() { return None; }
    // Field 22 of /proc/<pid>/stat, counted after the parenthesized command name.
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(')')?.1.split_whitespace().nth(19)?.parse().ok()
}

#[cfg(target_os = "macos")]
pub fn process_scope() -> Option<String> {
    let mut bytes = [0u8; 64];
    let mut len = bytes.len();
    let result = unsafe { libc::sysctlbyname(c"kern.bootsessionuuid".as_ptr(), bytes.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0) };
    if result != 0 || len > bytes.len() { return None; }
    mac_boot_scope(&bytes[..len])
}

#[cfg(any(target_os = "macos", test))]
fn mac_boot_scope(bytes: &[u8]) -> Option<String> {
    if bytes.len() != 37 || bytes[36] != 0 { return None; }
    let uuid = std::str::from_utf8(&bytes[..36]).ok()?;
    if !uuid.bytes().enumerate().all(|(i, b)| if matches!(i, 8 | 13 | 18 | 23) { b == b'-' } else { b.is_ascii_hexdigit() }) { return None; }
    if uuid.bytes().all(|b| b == b'0' || b == b'-') { return None; }
    Some(format!("macos-boot:{}", uuid.to_ascii_lowercase()))
}

#[cfg(target_os = "linux")]
fn proc_self_matches() -> bool {
    std::fs::read_link("/proc/self").ok().and_then(|path| path.to_str().and_then(|s| s.parse::<u32>().ok())) == Some(std::process::id())
}

/// The boot and PID namespace in which a stored Linux process ID is meaningful.
#[cfg(target_os = "linux")]
pub fn process_scope() -> Option<String> {
    if !proc_self_matches() { return None; }
    let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
    let boot = boot.trim();
    if boot.len() != 36 || !boot.chars().all(|c| c.is_ascii_hexdigit() || c == '-') { return None; }
    let namespace = std::fs::read_link("/proc/self/ns/pid").ok()?;
    if std::fs::read_link("/proc/1/ns/pid").ok()? != namespace { return None; }
    let namespace = namespace.to_str()?.strip_prefix("pid:[")?.strip_suffix(']')?;
    if namespace.is_empty() || !namespace.bytes().all(|c| c.is_ascii_digit()) { return None; }
    Some(format!("{boot}:{namespace}"))
}

/// Ends a process and everything it started (`taskkill /T /F` on Windows).
pub fn kill_tree(pid: u32) {
    let mut command = if cfg!(windows) {
        let mut c = Command::new("taskkill");
        c.args(["/PID", &pid.to_string(), "/T", "/F"]);
        c
    } else {
        let mut c = Command::new("kill");
        c.args(["-TERM", &pid.to_string()]);
        c
    };
    // Already gone between listing and stopping is fine.
    let _ = hide_window(&mut command).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
}

/// Exit code as Node reports it: Windows codes are unsigned.
pub fn exit_code(status: std::process::ExitStatus) -> Option<i64> {
    let code = status.code()?;
    Some(if cfg!(windows) { i64::from(code as u32) } else { i64::from(code) })
}

/// Appends one argument to a Windows command line the way the C runtime reads it back.
fn quote_arg(arg: &str, line: &mut String) {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\x0b', '"']) {
        line.push_str(arg);
        return;
    }
    line.push('"');
    let mut backslashes = 0;
    for c in arg.chars() {
        if c == '\\' {
            backslashes += 1;
        } else {
            if c == '"' {
                // Backslashes before a quote are doubled, and the quote escaped.
                line.extend(std::iter::repeat_n('\\', backslashes + 1));
            }
            backslashes = 0;
        }
        line.push(c);
    }
    line.extend(std::iter::repeat_n('\\', backslashes));
    line.push('"');
}

/// The command line `CreateProcessW` gets: the program in quotes, then each argument.
pub fn command_line(program: &Path, args: &[String]) -> String {
    let mut line = format!("\"{}\"", program.to_string_lossy());
    for arg in args {
        line.push(' ');
        quote_arg(arg, &mut line);
    }
    line
}

/// Starts a program in a visible console window of its own and does not wait:
/// for sign-ins a person runs. The program gets that console's input and output
/// (never this process's pipes), inherits no handles, and gets `env` as its whole
/// environment when one is given.
#[cfg(windows)]
pub fn spawn_console(program: &Path, args: &[String], env: Option<&BTreeMap<String, String>>, cwd: Option<&Path>) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{CreateProcessW, CREATE_NEW_CONSOLE, CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION, STARTUPINFOW};

    let wide = |text: &OsStr| text.encode_wide().chain(Some(0)).collect::<Vec<u16>>();
    let application = wide(program.as_os_str());
    let mut line = wide(OsStr::new(&command_line(program, args)));
    let block = env.map(|vars| {
        let mut block: Vec<u16> = vars.iter().flat_map(|(name, value)| wide(OsStr::new(&format!("{name}={value}")))).collect();
        block.push(0);
        if block.len() == 1 {
            block.push(0);
        }
        block
    });
    let cwd = cwd.map(|dir| wide(dir.as_os_str()));
    unsafe {
        let mut startup: STARTUPINFOW = std::mem::zeroed();
        startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        let mut info: PROCESS_INFORMATION = std::mem::zeroed();
        let ok = CreateProcessW(
            application.as_ptr(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_NEW_CONSOLE | CREATE_UNICODE_ENVIRONMENT,
            block.as_ref().map_or(std::ptr::null(), |b| b.as_ptr().cast()),
            cwd.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
            &startup,
            &mut info,
        );
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        CloseHandle(info.hThread);
        CloseHandle(info.hProcess);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn spawn_console(program: &Path, args: &[String], env: Option<&BTreeMap<String, String>>, cwd: Option<&Path>) -> io::Result<()> {
    super::macos_terminal::open(program, args, env, cwd)
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn spawn_console(program: &Path, args: &[String], env: Option<&BTreeMap<String, String>>, cwd: Option<&Path>) -> io::Result<()> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(env) = env {
        command.env_clear().envs(env);
    }
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    command.spawn().map(drop)
}

/// Opens a web page in the person's default browser.
#[cfg(windows)]
pub fn open_url(url: &str) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let wide = |text: &str| OsStr::new(text).encode_wide().chain(Some(0)).collect::<Vec<u16>>();
    let (verb, target) = (wide("open"), wide(url));
    let code = unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), target.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) };
    // Values above 32 mean success.
    if code as isize > 32 {
        Ok(())
    } else {
        Err(io::Error::other(format!("브라우저를 열지 못했습니다 (코드 {})", code as isize)))
    }
}

#[cfg(not(windows))]
pub fn open_url(url: &str) -> io::Result<()> {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    Command::new(opener).arg(url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map(drop)
}

#[cfg(test)]
mod mac_owner_scope_tests {
    use super::mac_boot_scope;
    #[test]
    fn only_complete_nonzero_boot_uuids_are_accepted() {
        assert_eq!(mac_boot_scope(b"12345678-ABCD-1234-abcd-1234567890ab\0").as_deref(), Some("macos-boot:12345678-abcd-1234-abcd-1234567890ab"));
        assert_eq!(mac_boot_scope(b"12345678-ABCD-1234-abcd-1234567890ab"), None);
        assert_eq!(mac_boot_scope(b"00000000-0000-0000-0000-000000000000\0"), None);
        assert_eq!(mac_boot_scope(b"12345678_ABCD-1234-abcd-1234567890ab\0"), None);
        assert_eq!(mac_boot_scope(b"12345678-ABCD-1234-abcd-1234567890az\0"), None);
    }
}
