use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn quote(value: &str) -> String { format!("'{}'", value.replace('\'', "'\\''")) }

fn script(program: &Path, args: &[String], env: Option<&BTreeMap<String, String>>, cwd: Option<&Path>) -> String {
    // Delete before login starts: the file contains only launch parameters, never credentials.
    let mut text = String::from("#!/bin/sh\n/bin/rm -f -- \"$0\"\n");
    if let Some(cwd) = cwd { text.push_str(&format!("cd {} || exit 1\n", quote(&cwd.to_string_lossy()))); }
    text.push_str("exec ");
    if let Some(env) = env {
        text.push_str("/usr/bin/env -i ");
        for (key, value) in env { text.push_str(&format!("{} ", quote(&format!("{key}={value}")))); }
    }
    text.push_str(&quote(&program.to_string_lossy()));
    for arg in args { text.push_str(&format!(" {}", quote(arg))); }
    text.push('\n');
    text
}

pub fn spawn(program: &Path, args: &[String], env: Option<&BTreeMap<String, String>>, cwd: Option<&Path>) -> io::Result<()> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(io::Error::other)?.as_nanos();
    let path = std::env::temp_dir().join(format!("project-o-login-{}-{stamp}.command", std::process::id()));
    let mut file = OpenOptions::new().write(true).create_new(true).mode(0o700).open(&path)?;
    let result = (|| {
        file.write_all(script(program, args, env, cwd).as_bytes())?;
        drop(file);
        let status = Command::new("/usr/bin/open").args(["-a", "Terminal"]).arg(&path).status()?;
        if !status.success() { return Err(io::Error::other("Terminal 창을 열지 못했습니다")); }
        Ok(())
    })();
    if result.is_err() { let _ = fs::remove_file(&path); }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_script_preserves_arguments_without_shell_expansion() {
        let dir = std::env::temp_dir().join(format!("o-console-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.command");
        let arg = "spaces 'quotes' $(touch SHOULD_NOT_EXIST) `id` $HOME";
        fs::write(&path, script(Path::new("/usr/bin/printf"), &["%s".into(), arg.into()], Some(&BTreeMap::new()), Some(&dir))).unwrap();
        let out = Command::new("/bin/sh").arg(&path).output().unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8(out.stdout).unwrap(), arg);
        assert!(!path.exists());
        assert!(!dir.join("SHOULD_NOT_EXIST").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
