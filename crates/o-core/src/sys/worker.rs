//! A worker's owned process and pipes, independent of the platform launcher.
use std::io::{self, Read, Write};
use std::process::ExitStatus;
use std::sync::{Arc, Mutex};

use super::proc::{self, KillOnClose};

pub struct Child {
    pub stdin: Option<Box<dyn Write + Send>>,
    pub stdout: Option<Box<dyn Read + Send>>,
    pub stderr: Option<Box<dyn Read + Send>>,
    inner: Inner,
}

enum Inner {
    Standard(std::process::Child),
    #[cfg(target_os = "macos")]
    Mac(super::macos_worker::Child),
    #[cfg(windows)]
    Native(super::win_launch::Child),
}

impl Child {
    pub fn standard(mut child: std::process::Child) -> Self {
        Self {
            stdin: child.stdin.take().map(|p| Box::new(p) as Box<dyn Write + Send>),
            stdout: child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>),
            stderr: child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>),
            inner: Inner::Standard(child),
        }
    }

    #[cfg(windows)]
    pub fn native(mut child: super::win_launch::Child) -> Self {
        Self {
            stdin: child.stdin.take().map(|p| Box::new(p) as Box<dyn Write + Send>),
            stdout: child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>),
            stderr: child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>),
            inner: Inner::Native(child),
        }
    }

    #[cfg(target_os = "macos")]
    pub fn mac(mut child: super::macos_worker::Child) -> Self {
        Self {
            stdin: child.stdin.take().map(|p| Box::new(p) as Box<dyn Write + Send>),
            stdout: child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>),
            stderr: child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>),
            inner: Inner::Mac(child),
        }
    }

    pub fn id(&self) -> u32 { match &self.inner { Inner::Standard(child) => child.id(), #[cfg(target_os = "macos")] Inner::Mac(child) => child.id(), #[cfg(windows)] Inner::Native(child) => child.id() } }
    #[cfg(windows)]
    pub fn watch_handle(&self) -> io::Result<std::os::windows::io::OwnedHandle> {
        use std::os::windows::io::{AsRawHandle, FromRawHandle};
        use windows_sys::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS};
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        match &self.inner {
            Inner::Native(child) => child.watch_handle(),
            Inner::Standard(child) => {
                let mut duplicate = std::ptr::null_mut();
                if unsafe { DuplicateHandle(GetCurrentProcess(), child.as_raw_handle(), GetCurrentProcess(), &mut duplicate, 0, 0, DUPLICATE_SAME_ACCESS) } == 0 { return Err(io::Error::last_os_error()); }
                Ok(unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(duplicate) })
            }
        }
    }
    pub fn resume(&self) -> io::Result<()> { match &self.inner { Inner::Standard(child) => proc::resume_worker(child.id()), #[cfg(target_os = "macos")] Inner::Mac(child) => child.resume(), #[cfg(windows)] Inner::Native(child) => child.resume() } }
    pub fn kill(&mut self) -> io::Result<()> { match &mut self.inner { Inner::Standard(child) => child.kill(), #[cfg(target_os = "macos")] Inner::Mac(child) => child.kill(), #[cfg(windows)] Inner::Native(child) => child.kill() } }
    pub fn try_wait(&mut self, tree: &Option<Arc<Mutex<KillOnClose>>>) -> io::Result<Option<ExitStatus>> {
        match &mut self.inner { Inner::Standard(child) => proc::try_wait_worker(child, tree), #[cfg(target_os = "macos")] Inner::Mac(child) => child.try_wait(), #[cfg(windows)] Inner::Native(child) => child.try_wait() }
    }
    pub fn wait(&mut self, tree: &Option<Arc<Mutex<KillOnClose>>>) -> io::Result<ExitStatus> {
        self.stdin.take();
        match &mut self.inner { Inner::Standard(child) => proc::wait_worker(child, tree), #[cfg(target_os = "macos")] Inner::Mac(child) => child.wait(), #[cfg(windows)] Inner::Native(child) => child.wait() }
    }
}
