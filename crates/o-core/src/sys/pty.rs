//! A hidden pseudo-console whose child belongs to a kill-on-close job.

use std::io;
use super::launch::Launch;

#[cfg(windows)]
mod windows {
    use super::*;
    use std::fs::File;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Console::{ClosePseudoConsole, CreatePseudoConsole, ResizePseudoConsole, COORD, HPCON};
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::{CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess, InitializeProcThreadAttributeList, UpdateProcThreadAttribute, WaitForSingleObject, CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_JOB_LIST, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, STARTF_USESTDHANDLES, STARTUPINFOEXW};

    struct Console(HPCON);
    impl Drop for Console { fn drop(&mut self) { unsafe { ClosePseudoConsole(self.0) } } }

    struct Attributes(Vec<usize>);
    impl Drop for Attributes { fn drop(&mut self) { unsafe { DeleteProcThreadAttributeList(self.0.as_mut_ptr().cast()) } } }

    pub struct Pty {
        console: Console,
        process: OwnedHandle,
        job: crate::sys::proc::KillOnClose,
        pid: u32,
        pub input: Option<File>,
        pub output: Option<File>,
    }

    fn pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
        let (mut read, mut write) = (std::ptr::null_mut(), std::ptr::null_mut());
        if unsafe { CreatePipe(&mut read, &mut write, std::ptr::null(), 0) } == 0 { return Err(io::Error::last_os_error()); }
        Ok(unsafe { (OwnedHandle::from_raw_handle(read), OwnedHandle::from_raw_handle(write)) })
    }

    fn size(cols: u16, rows: u16) -> io::Result<COORD> {
        if cols == 0 || rows == 0 || cols > i16::MAX as u16 || rows > i16::MAX as u16 { return Err(io::Error::new(io::ErrorKind::InvalidInput, "잘못된 터미널 크기")); }
        Ok(COORD { X: cols as i16, Y: rows as i16 })
    }

    impl Pty {
        pub fn spawn(launch: &Launch, cols: u16, rows: u16, label: Option<&str>) -> io::Result<Self> {
            let native = launch.native(std::env::vars_os())?;
            if !native.cwd.is_absolute() { return Err(io::Error::new(io::ErrorKind::InvalidInput, "absolute working directory required")); }
            let (pty_read, input) = pipe()?;
            let (output, pty_write) = pipe()?;
            let mut handle = 0;
            let result = unsafe { CreatePseudoConsole(size(cols, rows)?, pty_read.as_raw_handle(), pty_write.as_raw_handle(), 0, &mut handle) };
            if result < 0 { return Err(io::Error::from_raw_os_error(result)); }
            let console = Console(handle);
            // A card's terminal is in a card's job, a lead's in a lead's (`agent_job`, ADR-0043).
            let name = match label { Some(label) => crate::sys::win_launch::card_job_name(label), None => crate::sys::win_launch::lead_job_name() };
            let (job, job_name) = crate::sys::win_launch::kill_on_close_job(name)?;
            let mut bytes = 0;
            unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 2, 0, &mut bytes); }
            if bytes == 0 { return Err(io::Error::last_os_error()); }
            let mut storage = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
            let attrs = storage.as_mut_ptr().cast();
            if unsafe { InitializeProcThreadAttributeList(attrs, 2, 0, &mut bytes) } == 0 { return Err(io::Error::last_os_error()); }
            let mut attrs = Attributes(storage);
            let ptr = attrs.0.as_mut_ptr().cast();
            if unsafe { UpdateProcThreadAttribute(ptr, 0, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize, handle as *const _, std::mem::size_of::<HPCON>(), std::ptr::null_mut(), std::ptr::null()) } == 0 { return Err(io::Error::last_os_error()); }
            let jobs = [job.as_raw_handle()];
            if unsafe { UpdateProcThreadAttribute(ptr, 0, PROC_THREAD_ATTRIBUTE_JOB_LIST as usize, jobs.as_ptr().cast(), std::mem::size_of_val(&jobs), std::ptr::null_mut(), std::ptr::null()) } == 0 { return Err(io::Error::last_os_error()); }
            let mut startup = STARTUPINFOEXW::default();
            startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
            startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            startup.lpAttributeList = ptr;
            let mut info = PROCESS_INFORMATION::default();
            let app: Vec<u16> = native.executable.as_os_str().encode_wide().chain(Some(0)).collect();
            let cwd: Vec<u16> = native.cwd.as_os_str().encode_wide().chain(Some(0)).collect();
            let mut command = native.command_line()?;
            let environment = native.environment_block()?;
            if unsafe { CreateProcessW(app.as_ptr(), command.as_mut_ptr(), std::ptr::null(), std::ptr::null(), 0, EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT, environment.as_ptr().cast(), cwd.as_ptr(), &startup.StartupInfo, &mut info) } == 0 { return Err(io::Error::last_os_error()); }
            drop((pty_read, pty_write));
            let process = unsafe { OwnedHandle::from_raw_handle(info.hProcess) };
            let _thread = unsafe { OwnedHandle::from_raw_handle(info.hThread) };
            Ok(Self { console, process, job: crate::sys::proc::KillOnClose { job: job.into_raw_handle(), job_name }, pid: info.dwProcessId, input: Some(input.into()), output: Some(output.into()) })
        }

        pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
            let result = unsafe { ResizePseudoConsole(self.console.0, size(cols, rows)?) };
            if result < 0 { Err(io::Error::from_raw_os_error(result)) } else { Ok(()) }
        }
        pub fn try_wait(&self) -> io::Result<Option<i64>> {
            match unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) } {
                WAIT_TIMEOUT => Ok(None),
                WAIT_OBJECT_0 => { let mut code = 0; if unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &mut code) } == 0 { Err(io::Error::last_os_error()) } else { Ok(Some(i64::from(code))) } }
                _ => Err(io::Error::last_os_error()),
            }
        }
        pub fn kill(&self) -> io::Result<()> {
            use windows_sys::Win32::System::JobObjects::TerminateJobObject;
            if unsafe { TerminateJobObject(self.job.job, 1) } == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
        }
        pub fn pid(&self) -> u32 { self.pid }
        pub fn job_name(&self) -> Option<&str> { self.job.job_name() }
        pub fn stop_and_confirm(&mut self) -> bool {
            if !self.job.stop_and_confirm() { return false; }
            for _ in 0..100 {
                match self.try_wait() { Ok(Some(_)) => return true, Err(_) => return false, Ok(None) => std::thread::sleep(std::time::Duration::from_millis(25)) }
            }
            false
        }
    }
}

#[cfg(windows)]
pub use windows::Pty;

#[cfg(not(windows))]
pub struct Pty;
#[cfg(not(windows))]
impl Pty {
    pub fn spawn(_: &Launch, _: u16, _: u16, _: Option<&str>) -> io::Result<Self> { Err(io::Error::new(io::ErrorKind::Unsupported, "이 운영체제에서는 터미널을 지원하지 않습니다")) }
}
