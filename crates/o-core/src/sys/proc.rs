//! Process plumbing: hidden windows, detached runners, kill-on-close job
//! objects, short captured commands, liveness checks and tree kills. Windows
//! first; elsewhere the nearest equivalent.

use std::collections::BTreeMap;
#[cfg(windows)]
use std::ffi::OsStr;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
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

/// Holds a child's process tree. Dropping it (or this process dying) kills
/// whatever is still in the tree, so a crashed runner leaves no orphans.
pub struct KillOnClose {
    #[cfg(windows)]
    job: windows_sys::Win32::Foundation::HANDLE,
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

/// Puts a freshly spawned child in a job object that kills it, and every
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

#[cfg(not(windows))]
pub fn contain(_child: &Child) -> Option<KillOnClose> {
    None
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

#[cfg(not(windows))]
pub fn is_alive(pid: u32) -> bool {
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

#[cfg(not(windows))]
pub fn started_at(pid: u32) -> Option<u64> {
    // Field 22 of /proc/<pid>/stat, counted after the parenthesized command name.
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(')')?.1.split_whitespace().nth(19)?.parse().ok()
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
