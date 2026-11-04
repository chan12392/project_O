//! Explicit worker inputs: the environment baseline is part of the launch contract.
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnvironmentBase {
    Inherit,
    Replace,
}

#[derive(Clone, Debug)]
pub struct Launch {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
    pub environment_base: EnvironmentBase,
    pub environment_changes: Vec<(OsString, Option<OsString>)>,
    #[cfg(windows)]
    pub handle_owner_executable: Option<PathBuf>,
}

impl Launch {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self { executable: executable.into(), args: Vec::new(), cwd: PathBuf::new(), environment_base: EnvironmentBase::Inherit, environment_changes: Vec::new(), #[cfg(windows)] handle_owner_executable: None }
    }

    pub fn arg(&mut self, arg: impl Into<OsString>) -> &mut Self { self.args.push(arg.into()); self }
    pub fn args<I, S>(&mut self, args: I) -> &mut Self where I: IntoIterator<Item = S>, S: Into<OsString> {
        self.args.extend(args.into_iter().map(Into::into)); self
    }
    pub fn current_dir(&mut self, cwd: impl Into<PathBuf>) -> &mut Self { self.cwd = cwd.into(); self }
    pub fn env(&mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> &mut Self {
        self.environment_changes.push((name.into(), Some(value.into()))); self
    }
    pub fn envs<I, K, V>(&mut self, env: I) -> &mut Self where I: IntoIterator<Item = (K, V)>, K: Into<OsString>, V: Into<OsString> {
        for (name, value) in env { self.env(name, value); } self
    }
    pub fn env_remove(&mut self, name: impl Into<OsString>) -> &mut Self { self.environment_changes.push((name.into(), None)); self }
    pub fn env_clear(&mut self) -> &mut Self { self.environment_base = EnvironmentBase::Replace; self.environment_changes.clear(); self }

    pub fn command(&self) -> std::process::Command {
        let mut command = std::process::Command::new(&self.executable);
        command.args(&self.args);
        if !self.cwd.as_os_str().is_empty() { command.current_dir(&self.cwd); }
        if self.environment_base == EnvironmentBase::Replace { command.env_clear(); }
        for (name, value) in &self.environment_changes {
            match value { Some(value) => { command.env(name, value); }, None => { command.env_remove(name); } }
        }
        command
    }

    /// Apply changes in order, just as process environment updates do.
    pub fn environment(&self, inherited: impl IntoIterator<Item = (OsString, OsString)>) -> io::Result<Vec<(OsString, OsString)>> {
        let mut result: Vec<_> = match self.environment_base {
            EnvironmentBase::Inherit => inherited.into_iter().collect(),
            EnvironmentBase::Replace => Vec::new(),
        };
        for (name, value) in &self.environment_changes {
            let mut retained = Vec::with_capacity(result.len());
            for (existing, old_value) in result {
                if !same_name(&existing, name)? { retained.push((existing, old_value)); }
            }
            result = retained;
            if let Some(value) = value { result.push((name.clone(), value.clone())); }
        }
        Ok(result)
    }

    #[cfg(windows)]
    pub fn native(&self, inherited: impl IntoIterator<Item = (OsString, OsString)>) -> io::Result<super::win_launch::Launch> {
        Ok(super::win_launch::Launch {
            executable: self.executable.clone(), args: self.args.clone(), cwd: if self.cwd.as_os_str().is_empty() { std::env::current_dir()? } else { self.cwd.clone() },
            environment: self.environment(inherited)?,
        })
    }
}

#[cfg(windows)]
fn same_name(a: &OsStr, b: &OsStr) -> io::Result<bool> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Globalization::CompareStringOrdinal;
    let a: Vec<u16> = a.encode_wide().collect();
    let b: Vec<u16> = b.encode_wide().collect();
    if a.len() > i32::MAX as usize || b.len() > i32::MAX as usize { return Err(io::Error::new(io::ErrorKind::InvalidInput, "environment name too long")); }
    let compared = unsafe { CompareStringOrdinal(a.as_ptr(), a.len() as i32, b.as_ptr(), b.len() as i32, 1) };
    if compared == 0 { Err(io::Error::last_os_error()) } else { Ok(compared == 2) }
}

#[cfg(not(windows))]
fn same_name(a: &OsStr, b: &OsStr) -> io::Result<bool> { Ok(a == b) }
