//! Explicit Windows process launch data. This module is not yet wired to the runner.
use std::cmp::Ordering;
use std::ffi::{OsStr, OsString};
use std::io;
use std::fs::File;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle};
use std::os::windows::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::ExitStatus;
use windows_sys::Win32::Foundation::{DuplicateHandle, GetLastError, DUPLICATE_CLOSE_SOURCE, DUPLICATE_SAME_ACCESS, ERROR_ALREADY_EXISTS, FILETIME, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::Globalization::CompareStringOrdinal;
use windows_sys::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};
use windows_sys::Win32::System::JobObjects::{CreateJobObjectW, SetInformationJobObject, JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{CreateEventW, CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess, GetExitCodeProcess, GetProcessTimes, InitializeProcThreadAttributeList, OpenEventW, OpenProcess, ResumeThread, SetEvent, TerminateProcess, UpdateProcThreadAttribute, WaitForMultipleObjects, WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, EVENT_MODIFY_STATE, EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_JOB_LIST, PROC_THREAD_ATTRIBUTE_PARENT_PROCESS, STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW};

/// Complete launch inputs: callers must supply the already-filtered environment.
pub struct Launch {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
    pub environment: Vec<(OsString, OsString)>,
}

/// Prototype handle-only process, started without inheriting any caller handles.
pub struct HandleOwner { process: OwnedHandle }

impl AsRawHandle for HandleOwner {
    fn as_raw_handle(&self) -> std::os::windows::io::RawHandle { self.process.as_raw_handle() }
}

impl Drop for HandleOwner {
    fn drop(&mut self) {
        unsafe { TerminateProcess(self.process.as_raw_handle(), 1); WaitForSingleObject(self.process.as_raw_handle(), 5_000); }
    }
}

impl HandleOwner {
    /// The executable must dispatch `flag` to `run_handle_owner` before normal initialization.
    pub fn start(executable: PathBuf, flag: &str) -> io::Result<Self> {
        let mut random = [0u8; 16];
        if unsafe { BCryptGenRandom(std::ptr::null_mut(), random.as_mut_ptr(), random.len() as u32, BCRYPT_USE_SYSTEM_PREFERRED_RNG) } < 0 {
            return Err(io::Error::other("handle-owner nonce generation failed"));
        }
        let name = format!("Local\\ProjectO-HandleOwner-{}", random.iter().map(|b| format!("{b:02x}")).collect::<String>());
        let mut event_name: Vec<u16> = name.encode_utf16().collect(); event_name.push(0);
        let raw_event = unsafe { CreateEventW(std::ptr::null(), 1, 0, event_name.as_ptr()) };
        if raw_event.is_null() { return Err(io::Error::last_os_error()); }
        let event = unsafe { OwnedHandle::from_raw_handle(raw_event) };
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS { return Err(io::Error::other("handle-owner event already exists")); }
        let pid = std::process::id();
        let created = super::proc::started_at(pid).ok_or_else(|| io::Error::other("owner identity unavailable"))?;
        let mut environment = Vec::new();
        if let Some(root) = std::env::var_os("SystemRoot") { environment.push((OsString::from("SystemRoot"), root)); }
        let launch = Launch { executable, args: vec![flag.into(), pid.to_string().into(), created.to_string().into(), name.into()], cwd: std::env::current_dir()?, environment };
        let mut command = launch.command_line()?;
        let mut app = wide(launch.executable.as_os_str())?; app.push(0);
        let env = launch.environment_block()?;
        let startup = STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, ..Default::default() };
        let mut info = PROCESS_INFORMATION::default();
        if unsafe { CreateProcessW(app.as_ptr(), command.as_mut_ptr(), std::ptr::null(), std::ptr::null(), 0, CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT, env.as_ptr().cast(), std::ptr::null(), &startup, &mut info) } == 0 { return Err(io::Error::last_os_error()); }
        let process = unsafe { OwnedHandle::from_raw_handle(info.hProcess) };
        let _thread = unsafe { OwnedHandle::from_raw_handle(info.hThread) };
        let owner = Self { process };
        // A stopped helper wins over a simultaneous readiness signal.
        let handles = [owner.as_raw_handle(), event.as_raw_handle()];
        match unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, 10_000) } {
            result if result == WAIT_OBJECT_0 + 1 => Ok(owner),
            WAIT_TIMEOUT => Err(io::Error::new(io::ErrorKind::TimedOut, "handle owner did not become ready")),
            WAIT_OBJECT_0 => Err(io::Error::other("handle owner exited before readiness")),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

/// Entry point for a trusted handle-only process; it never reads provider state.
pub fn run_handle_owner(args: &[OsString]) -> io::Result<()> {
    if args.len() != 3 { return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid handle-owner arguments")); }
    let pid: u32 = args[0].to_str().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid owner pid"))?.parse().map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid owner pid"))?;
    let expected: u64 = args[1].to_str().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid owner start"))?.parse().map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid owner start"))?;
    let raw_parent = unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if raw_parent.is_null() { return Err(io::Error::last_os_error()); }
    let parent = unsafe { OwnedHandle::from_raw_handle(raw_parent) };
    let (mut created, mut exited, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    if unsafe { GetProcessTimes(parent.as_raw_handle(), &mut created, &mut exited, &mut kernel, &mut user) } == 0 { return Err(io::Error::last_os_error()); }
    if ((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime)) != expected { return Err(io::Error::other("handle-owner parent identity changed")); }
    let mut event_name = wide(&args[2])?; event_name.push(0);
    let raw_event = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, event_name.as_ptr()) };
    if raw_event.is_null() { return Err(io::Error::last_os_error()); }
    let event = unsafe { OwnedHandle::from_raw_handle(raw_event) };
    if unsafe { SetEvent(event.as_raw_handle()) } == 0 { return Err(io::Error::last_os_error()); }
    if unsafe { WaitForSingleObject(parent.as_raw_handle(), u32::MAX) } != WAIT_OBJECT_0 { return Err(io::Error::last_os_error()); }
    Ok(())
}

/// Owns the exact process and its initial thread; the job is returned separately.
pub struct Child {
    pub stdin: Option<File>,
    pub stdout: Option<File>,
    pub stderr: Option<File>,
    process: OwnedHandle,
    thread: OwnedHandle,
    id: u32,
}

impl Child {
    pub fn id(&self) -> u32 { self.id }

    pub fn watch_handle(&self) -> io::Result<OwnedHandle> {
        let mut duplicate = std::ptr::null_mut();
        if unsafe { DuplicateHandle(GetCurrentProcess(), self.process.as_raw_handle(), GetCurrentProcess(), &mut duplicate, 0, 0, DUPLICATE_SAME_ACCESS) } == 0 { return Err(io::Error::last_os_error()); }
        Ok(unsafe { OwnedHandle::from_raw_handle(duplicate) })
    }

    pub fn resume(&self) -> io::Result<()> {
        let previous = unsafe { ResumeThread(self.thread.as_raw_handle()) };
        if previous == 1 { Ok(()) } else if previous == u32::MAX { Err(io::Error::last_os_error()) } else { Err(io::Error::other("unexpected thread suspend count")) }
    }

    pub fn kill(&self) -> io::Result<()> {
        if unsafe { TerminateProcess(self.process.as_raw_handle(), 1) } == 0 { return Err(io::Error::last_os_error()); }
        Ok(())
    }

    pub fn try_wait(&self) -> io::Result<Option<ExitStatus>> {
        match unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) } {
            WAIT_TIMEOUT => Ok(None),
            WAIT_OBJECT_0 => {
                let mut code = 0;
                if unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &mut code) } == 0 { return Err(io::Error::last_os_error()); }
                Ok(Some(ExitStatus::from_raw(code)))
            }
            _ => Err(io::Error::last_os_error()),
        }
    }

    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        self.stdin.take();
        if unsafe { WaitForSingleObject(self.process.as_raw_handle(), u32::MAX) } != WAIT_OBJECT_0 { return Err(io::Error::last_os_error()); }
        self.try_wait()?.ok_or_else(|| io::Error::other("process exit unavailable"))
    }
}

struct Attributes { storage: Vec<usize>, initialized: bool }
impl Attributes {
    fn new() -> io::Result<Self> {
        let mut size = 0;
        unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 3, 0, &mut size) };
        if size == 0 { return Err(io::Error::last_os_error()); }
        let mut result = Self { storage: vec![0; size.div_ceil(std::mem::size_of::<usize>())], initialized: false };
        if unsafe { InitializeProcThreadAttributeList(result.ptr(), 3, 0, &mut size) } == 0 { return Err(io::Error::last_os_error()); }
        result.initialized = true;
        Ok(result)
    }
    fn ptr(&mut self) -> *mut std::ffi::c_void { self.storage.as_mut_ptr().cast() }
    fn set(&mut self, attr: u32, values: &[HANDLE]) -> io::Result<()> {
        if unsafe { UpdateProcThreadAttribute(self.ptr(), 0, attr as usize, values.as_ptr().cast(), std::mem::size_of_val(values), std::ptr::null_mut(), std::ptr::null()) } == 0 { return Err(io::Error::last_os_error()); }
        Ok(())
    }
}
impl Drop for Attributes {
    fn drop(&mut self) { if self.initialized { unsafe { DeleteProcThreadAttributeList(self.ptr()) }; } }
}

fn pipe(parent_reads: bool) -> io::Result<(OwnedHandle, OwnedHandle)> {
    let mut read = std::ptr::null_mut();
    let mut write = std::ptr::null_mut();
    if unsafe { CreatePipe(&mut read, &mut write, std::ptr::null(), 0) } == 0 { return Err(io::Error::last_os_error()); }
    let read = unsafe { OwnedHandle::from_raw_handle(read) };
    let write = unsafe { OwnedHandle::from_raw_handle(write) };
    let (parent, child) = if parent_reads { (read, write) } else { (write, read) };
    Ok((parent, child))
}

// The duplicated handles exist only in the designated parent, never the caller.
struct RemoteHandles { parent: HANDLE, handles: Vec<HANDLE> }
impl RemoteHandles {
    fn duplicate(&mut self, source: &OwnedHandle) -> io::Result<()> {
        let mut remote = std::ptr::null_mut();
        if unsafe { DuplicateHandle(GetCurrentProcess(), source.as_raw_handle(), self.parent, &mut remote, 0, 1, DUPLICATE_SAME_ACCESS) } == 0 { return Err(io::Error::last_os_error()); }
        self.handles.push(remote);
        Ok(())
    }
}
impl Drop for RemoteHandles {
    fn drop(&mut self) {
        for handle in self.handles.drain(..) {
            unsafe { DuplicateHandle(self.parent, handle, std::ptr::null_mut(), std::ptr::null_mut(), 0, 0, DUPLICATE_CLOSE_SOURCE); }
        }
    }
}

impl Launch {
    /// Create a suspended process already assigned to a kill-on-close job.
    pub fn spawn(&self, owner: &impl AsRawHandle) -> io::Result<(Child, super::proc::KillOnClose)> {
        self.spawn_with_hook(owner, || Ok(()))
    }

    /// Prototype hook allows tests to overlap an unrelated process creation.
    pub fn spawn_with_hook(&self, owner: &impl AsRawHandle, before_create: impl FnOnce() -> io::Result<()>) -> io::Result<(Child, super::proc::KillOnClose)> {
        self.spawn_with_hooks(owner, before_create, |_| {})
    }

    /// Prototype hooks expose the two sides of CreateProcessW for crash tests.
    pub fn spawn_with_hooks(&self, owner: &impl AsRawHandle, before_create: impl FnOnce() -> io::Result<()>, after_create: impl FnOnce(u32)) -> io::Result<(Child, super::proc::KillOnClose)> {
        if !self.cwd.is_absolute() { return Err(io::Error::new(io::ErrorKind::InvalidInput, "absolute working directory required")); }
        let mut command = self.command_line()?;
        let mut app = wide(self.executable.as_os_str())?; app.push(0);
        let mut cwd = wide(self.cwd.as_os_str())?; cwd.push(0);
        let environment = self.environment_block()?;
        let raw_job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if raw_job.is_null() { return Err(io::Error::last_os_error()); }
        let job = unsafe { OwnedHandle::from_raw_handle(raw_job) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe { SetInformationJobObject(raw_job, JobObjectExtendedLimitInformation, (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(), std::mem::size_of_val(&limits) as u32) } == 0 { return Err(io::Error::last_os_error()); }
        let (stdin, child_stdin) = pipe(false)?;
        let (stdout, child_stdout) = pipe(true)?;
        let (stderr, child_stderr) = pipe(true)?;
        let jobs = [raw_job];
        let parents = [owner.as_raw_handle()];
        let mut remote = RemoteHandles { parent: parents[0], handles: Vec::with_capacity(3) };
        remote.duplicate(&child_stdin)?;
        remote.duplicate(&child_stdout)?;
        remote.duplicate(&child_stderr)?;
        let handles = [remote.handles[0], remote.handles[1], remote.handles[2]];
        let mut attrs = Attributes::new()?;
        attrs.set(PROC_THREAD_ATTRIBUTE_JOB_LIST, &jobs)?;
        attrs.set(PROC_THREAD_ATTRIBUTE_PARENT_PROCESS, &parents)?;
        attrs.set(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &handles)?;
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = handles[0];
        startup.StartupInfo.hStdOutput = handles[1];
        startup.StartupInfo.hStdError = handles[2];
        startup.lpAttributeList = attrs.ptr();
        let mut info = PROCESS_INFORMATION::default();
        before_create()?;
        if unsafe { CreateProcessW(app.as_ptr(), command.as_mut_ptr(), std::ptr::null(), std::ptr::null(), 1, CREATE_NO_WINDOW | CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT, environment.as_ptr().cast(), cwd.as_ptr(), &startup.StartupInfo, &mut info) } == 0 { return Err(io::Error::last_os_error()); }
        let process = unsafe { OwnedHandle::from_raw_handle(info.hProcess) };
        let thread = unsafe { OwnedHandle::from_raw_handle(info.hThread) };
        after_create(info.dwProcessId);
        let child = Child { stdin: Some(stdin.into()), stdout: Some(stdout.into()), stderr: Some(stderr.into()), process, thread, id: info.dwProcessId };
        Ok((child, super::proc::KillOnClose { job: job.into_raw_handle() }))
    }
}

fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let result: Vec<_> = value.encode_wide().collect();
    if result.contains(&0) { return Err(io::Error::new(io::ErrorKind::InvalidInput, "NUL in process input")); }
    Ok(result)
}

/// Encode one argument for the Windows C-runtime argument parser, without a shell.
fn append_arg(out: &mut Vec<u16>, value: &OsStr) -> io::Result<()> {
    let value = wide(value)?;
    out.push(b'"' as u16);
    let mut slashes = 0;
    for ch in value {
        match ch {
            0x5c => slashes += 1,
            0x22 => {
                out.extend(std::iter::repeat_n(0x5c, slashes * 2 + 1));
                out.push(ch);
                slashes = 0;
            }
            _ => {
                out.extend(std::iter::repeat_n(0x5c, slashes));
                out.push(ch);
                slashes = 0;
            }
        }
    }
    out.extend(std::iter::repeat_n(0x5c, slashes * 2));
    out.push(b'"' as u16);
    Ok(())
}

impl Launch {
    /// Encode the command line for a native executable, never a batch interpreter.
    pub fn command_line(&self) -> io::Result<Vec<u16>> {
        if !self.executable.is_absolute() || self.executable.extension().is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat")) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "absolute native executable required"));
        }
        let mut command = Vec::new();
        append_arg(&mut command, self.executable.as_os_str())?;
        for arg in &self.args { command.push(b' ' as u16); append_arg(&mut command, arg)?; }
        command.push(0);
        Ok(command)
    }

    /// CreateProcessW requires case-insensitively sorted, doubly terminated UTF-16.
    pub fn environment_block(&self) -> io::Result<Vec<u16>> {
        let mut entries = Vec::with_capacity(self.environment.len());
        for (name, value) in &self.environment {
            let key = wide(name)?;
            // Windows ends a name at the first `=` after its first character, so cmd's hidden
            // variables (`=C:`, `=ExitCode`, `=::`) are valid names and inherited as they are.
            if key.is_empty() || key == [b'=' as u16] || key[1..].contains(&(b'=' as u16)) || key.len() > i32::MAX as usize {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid environment name"));
            }
            entries.push((key, wide(value)?));
        }
        let compare = |a: &[u16], b: &[u16]| -> Ordering {
            let result = unsafe { CompareStringOrdinal(a.as_ptr(), a.len() as i32, b.as_ptr(), b.len() as i32, 1) };
            match result { 1 => Ordering::Less, 3 => Ordering::Greater, _ => Ordering::Equal }
        };
        entries.sort_by(|a, b| compare(&a.0, &b.0));
        if entries.windows(2).any(|w| compare(&w[0].0, &w[1].0) == Ordering::Equal) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "duplicate environment name"));
        }
        let mut result = Vec::new();
        for (key, value) in entries { result.extend(key); result.push(b'=' as u16); result.extend(value); result.push(0); }
        if result.is_empty() { result.push(0); }
        result.push(0);
        Ok(result)
    }
}
