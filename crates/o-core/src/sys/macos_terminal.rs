//! Terminal receives only a socket path; the login environment stays off the command line.
use std::collections::BTreeMap;
use std::io;
#[cfg(target_os = "macos")]
use std::path::Path;

fn quote(text: &str) -> io::Result<String> {
    if text.contains('\0') { return Err(io::Error::other("터미널 실행 인자에 NUL이 있습니다")); }
    Ok(format!("'{}'", text.replace('\'', "'\\''")))
}

fn payload(program: &str, args: &[String], env: &BTreeMap<String, String>, cwd: &str) -> io::Result<String> {
    let mut script = format!("cd {} || exit 1\n", quote(cwd)?);
    for (key, value) in env {
        let mut bytes = key.bytes();
        if !bytes.next().is_some_and(|b| b == b'_' || b.is_ascii_alphabetic()) || !bytes.all(|b| b == b'_' || b.is_ascii_alphanumeric()) {
            return Err(io::Error::other("터미널 환경 변수 이름을 전달할 수 없습니다"));
        }
        script.push_str(&format!("export {key}={}\n", quote(value)?));
    }
    script.push_str("exec ");
    script.push_str(&quote(program)?);
    for arg in args { script.push(' '); script.push_str(&quote(arg)?); }
    script.push('\n');
    // shortcut: bound one launch's private transfer; report oversize instead of truncating.
    if script.len() > 256 * 1024 { return Err(io::Error::other("터미널 실행 환경이 너무 큽니다")); }
    Ok(script)
}

fn terminal_script(socket: &str) -> io::Result<String> {
    if socket.chars().any(char::is_control) { return Err(io::Error::other("터미널 통신 경로가 올바르지 않습니다")); }
    let shell = format!("eval \"$(/usr/bin/nc -d -U {})\"", quote(socket)?);
    let command = format!("exec /usr/bin/env -i /bin/sh -c {}", quote(&shell)?);
    let apple = command.replace('\\', "\\\\").replace('"', "\\\"");
    Ok(format!("tell application \"Terminal\"\n  do script \"{apple}\"\n  activate\nend tell\n"))
}

#[cfg(target_os = "macos")]
pub(super) fn open(program: &Path, args: &[String], env: Option<&BTreeMap<String, String>>, cwd: Option<&Path>) -> io::Result<()> {
    use std::fs::{self, DirBuilder};
    use std::io::Write;
    use std::os::unix::fs::DirBuilderExt;
    use std::os::unix::net::UnixListener;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    let program = program.to_str().ok_or_else(|| io::Error::other("터미널 실행 경로를 읽을 수 없습니다"))?;
    let current = std::env::current_dir()?;
    let directory = cwd.map_or_else(|| current.clone(), |path| if path.is_absolute() { path.to_owned() } else { current.join(path) });
    let directory = directory.to_str().ok_or_else(|| io::Error::other("터미널 작업 폴더를 읽을 수 없습니다"))?;
    if !Path::new(directory).is_dir() { return Err(io::Error::other("터미널 작업 폴더를 찾을 수 없습니다")); }
    let inherited = if env.is_none() {
        Some(std::env::vars_os().map(|(key, value)| {
            Ok((key.into_string().map_err(|_| io::Error::other("터미널 환경 이름을 읽을 수 없습니다"))?, value.into_string().map_err(|_| io::Error::other("터미널 환경 값을 읽을 수 없습니다"))?))
        }).collect::<io::Result<BTreeMap<_, _>>>()?)
    } else { None };
    let script = payload(program, args, env.or(inherited.as_ref()).expect("environment is present"), directory)?;

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(io::Error::other)?.as_nanos();
    let dir = std::env::temp_dir().join(format!("o{:x}{:x}{:x}", std::process::id(), stamp, SEQUENCE.fetch_add(1, Ordering::Relaxed)));
    DirBuilder::new().mode(0o700).create(&dir)?;
    struct Cleanup { dir: std::path::PathBuf, socket: std::path::PathBuf }
    impl Drop for Cleanup {
        fn drop(&mut self) { let _ = fs::remove_file(&self.socket); let _ = fs::remove_dir(&self.dir); }
    }
    let cleanup = Cleanup { socket: dir.join("s"), dir };
    let listener = UnixListener::bind(&cleanup.socket)?;
    listener.set_nonblocking(true)?;
    let request = terminal_script(cleanup.socket.to_str().ok_or_else(|| io::Error::other("터미널 통신 경로를 읽을 수 없습니다"))?)?;
    let mut apple = Command::new("/usr/bin/osascript").arg("-").stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    let written = apple.stdin.take().ok_or_else(|| io::Error::other("터미널 요청을 전달하지 못했습니다"))
        .and_then(|mut input| input.write_all(request.as_bytes()));
    if let Err(error) = written { let _ = apple.kill(); let _ = apple.wait(); return Err(error); }
    if !apple.wait()?.success() { return Err(io::Error::other("터미널 창을 열지 못했습니다. macOS 자동화 권한을 확인하세요")); }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_write_timeout(Some(Duration::from_secs(2)))?;
                stream.write_all(script.as_bytes())?;
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Err(io::Error::other("터미널 연결을 확인하지 못했습니다")),
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_arguments_and_keeps_secrets_out_of_terminal_request() {
        let mut env = BTreeMap::new();
        env.insert("HOME".into(), "/tmp/a ' b".into());
        env.insert("O_TEST".into(), "secret'$(touch bad)\nline".into());
        let text = payload("/a b/cli", &["a'b".into(), "$(bad)".into()], &env, "/tmp/x'y").unwrap();
        assert!(text.contains("cd '/tmp/x'\\''y' || exit 1"));
        assert!(text.contains("export O_TEST='secret'\\''$(touch bad)\nline'"));
        assert!(text.ends_with("exec '/a b/cli' 'a'\\''b' '$(bad)'\n"));
        let request = terminal_script("/tmp/socket 'x'").unwrap();
        assert!(!request.contains("secret") && !request.contains("touch bad"));
        assert!(request.contains("/usr/bin/env -i") && request.contains("/usr/bin/nc -d -U"));
    }

    #[test]
    fn refuses_invalid_environment_and_unrepresentable_input() {
        let mut env = BTreeMap::new();
        env.insert("BAD-NAME".into(), "secret".into());
        assert!(payload("cli", &[], &env, "/tmp").is_err());
        assert!(payload("cli\0bad", &[], &BTreeMap::new(), "/tmp").is_err());
        assert!(terminal_script("/tmp/bad\npath").is_err());
        env.clear();
        env.insert("BIG".into(), "x".repeat(256 * 1024));
        assert!(payload("cli", &[], &env, "/tmp").is_err());
    }

    #[test]
    fn shell_receives_exact_values_and_only_selected_environment() {
        let shell = if cfg!(windows) { r"C:\Program Files\Git\bin\bash.exe" } else { "/bin/sh" };
        if !std::path::Path::new(shell).is_file() { eprintln!("shell unavailable; payload execution not checked"); return; }
        let mut env = BTreeMap::new();
        env.insert("O_TEST".into(), "a'$(printf WRONG)\nvalue".into());
        let args = vec!["-c".into(), "printf '%s|%s|%s' \"$O_TEST\" \"$1\" \"${O_LEAK-unset}\"".into(), "arg0".into(), "quoted ' $(bad)".into()];
        let script = payload("/bin/sh", &args, &env, "/").unwrap();
        let command = format!("exec /usr/bin/env -i /bin/sh -c {}", quote(&script).unwrap());
        let output = std::process::Command::new(shell).args(["-c", &command]).env("O_LEAK", "hidden").output().unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "a'$(printf WRONG)\nvalue|quoted ' $(bad)|unset");
    }

    #[test]
    fn clean_profiles_use_the_supplied_root_and_discard_secrets() {
        use crate::agents::clean::{clean_env_from, profile_home};
        use crate::run::plan::ProviderId;
        let root = std::path::Path::new("/tmp/other ' root");
        for provider in [ProviderId::Claude, ProviderId::Codex] {
            let home = profile_home(root, provider);
            let parent = [("PATH".into(), "/bin".into()), ("O_SECRET".into(), "do-not-inherit".into())];
            let env = clean_env_from(parent, &home, provider);
            assert_eq!(env.get("HOME").map(String::as_str), home.to_str());
            assert!(!env.contains_key("O_SECRET"));
            let code = payload("/bin/agent", &["login".into()], &env, home.to_str().unwrap()).unwrap();
            assert!(code.contains(&quote(home.to_str().unwrap()).unwrap()));
            assert!(!code.contains("do-not-inherit"));
        }
    }
}
