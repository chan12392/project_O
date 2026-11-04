//! Explicit process inputs; arbitrary pre-exec hooks are intentionally not supported.
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnvironmentBase { Inherit, Replace }

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
    pub fn args<I, S>(&mut self, args: I) -> &mut Self where I: IntoIterator<Item = S>, S: Into<OsString> { self.args.extend(args.into_iter().map(Into::into)); self }
    pub fn current_dir(&mut self, cwd: impl Into<PathBuf>) -> &mut Self { self.cwd = cwd.into(); self }
    pub fn env(&mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> &mut Self { self.environment_changes.push((name.into(), Some(value.into()))); self }
    pub fn envs<I, K, V>(&mut self, env: I) -> &mut Self where I: IntoIterator<Item = (K, V)>, K: Into<OsString>, V: Into<OsString> { for (name, value) in env { self.env(name, value); } self }
    pub fn env_remove(&mut self, name: impl Into<OsString>) -> &mut Self { self.environment_changes.push((name.into(), None)); self }
    pub fn env_clear(&mut self) -> &mut Self { self.environment_base = EnvironmentBase::Replace; self.environment_changes.clear(); self }
    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        command.args(&self.args);
        if !self.cwd.as_os_str().is_empty() { command.current_dir(&self.cwd); }
        if self.environment_base == EnvironmentBase::Replace { command.env_clear(); }
        for (name, value) in &self.environment_changes {
            match value { Some(value) => { command.env(name, value); }, None => { command.env_remove(name); } }
        }
        command
    }

    #[cfg(windows)]
    pub fn native(&self, inherited: impl IntoIterator<Item = (OsString, OsString)>) -> std::io::Result<super::win_launch::Launch> {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Globalization::CompareStringOrdinal;
        let mut environment: Vec<_> = if self.environment_base == EnvironmentBase::Inherit { inherited.into_iter().collect() } else { Vec::new() };
        for (name, value) in &self.environment_changes {
            let key: Vec<u16> = name.encode_wide().collect();
            if key.len() > i32::MAX as usize { return Err(std::io::Error::other("environment name too long")); }
            let mut retained = Vec::with_capacity(environment.len());
            for (old, old_value) in environment {
                let candidate: Vec<u16> = old.encode_wide().collect();
                if candidate.len() > i32::MAX as usize { return Err(std::io::Error::other("environment name too long")); }
                let cmp = unsafe { CompareStringOrdinal(candidate.as_ptr(), candidate.len() as i32, key.as_ptr(), key.len() as i32, 1) };
                if cmp == 0 { return Err(std::io::Error::last_os_error()); }
                if cmp != 2 { retained.push((old, old_value)); }
            }
            environment = retained;
            if let Some(value) = value { environment.push((name.clone(), value.clone())); }
        }
        Ok(super::win_launch::Launch { executable: self.executable.clone(), args: self.args.clone(), cwd: if self.cwd.as_os_str().is_empty() { std::env::current_dir()? } else { self.cwd.clone() }, environment })
    }
}
