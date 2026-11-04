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

    pub fn id(&self) -> u32 { match &self.inner { Inner::Standard(child) => child.id(), #[cfg(windows)] Inner::Native(child) => child.id() } }
    pub fn resume(&self) -> io::Result<()> { match &self.inner { Inner::Standard(child) => proc::resume_worker(child.id()), #[cfg(windows)] Inner::Native(child) => child.resume() } }
    pub fn kill(&mut self) -> io::Result<()> { match &mut self.inner { Inner::Standard(child) => child.kill(), #[cfg(windows)] Inner::Native(child) => child.kill() } }
    pub fn try_wait(&mut self, tree: &Option<Arc<Mutex<KillOnClose>>>) -> io::Result<Option<ExitStatus>> {
        match &mut self.inner { Inner::Standard(child) => proc::try_wait_worker(child, tree), #[cfg(windows)] Inner::Native(child) => child.try_wait() }
    }
    pub fn wait(&mut self, tree: &Option<Arc<Mutex<KillOnClose>>>) -> io::Result<ExitStatus> {
        self.stdin.take();
        match &mut self.inner { Inner::Standard(child) => proc::wait_worker(child, tree), #[cfg(windows)] Inner::Native(child) => child.wait() }
    }
}
