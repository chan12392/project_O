//! Observe the owned worker before any provider code runs. Darwin cannot track
//! descendants with NOTE_TRACK; a fork permanently makes exit proof unknown.
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{launch::Launch, proc::KillOnClose};

pub struct Monitor {
    queue: OwnedFd,
    pid: u32,
    pub reaped: bool,
    exited: bool,
    uncertain: bool,
}

impl Monitor {
    fn new(pid: u32) -> io::Result<Self> {
        let fd = unsafe { libc::kqueue() };
        if fd < 0 { return Err(io::Error::last_os_error()); }
        let queue = unsafe { OwnedFd::from_raw_fd(fd) };
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 { return Err(io::Error::last_os_error()); }
        let event = libc::kevent { ident: pid as _, filter: libc::EVFILT_PROC,
            flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_CLEAR,
            fflags: libc::NOTE_EXIT | libc::NOTE_FORK, data: 0, udata: std::ptr::null_mut() };
        if unsafe { libc::kevent(fd, &event, 1, std::ptr::null_mut(), 0, std::ptr::null()) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { queue, pid, reaped: false, exited: false, uncertain: false })
    }

    fn poll(&mut self) {
        // Drain queued fork bits even if NOTE_EXIT was delivered in the same event.
        loop {
            let mut event: libc::kevent = unsafe { std::mem::zeroed() };
            let timeout = libc::timespec { tv_sec: 0, tv_nsec: 0 };
            let count = unsafe { libc::kevent(self.queue.as_raw_fd(), std::ptr::null(), 0, &mut event, 1, &timeout) };
            if count == 0 { break; }
            if count < 0 {
                if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted { continue; }
                self.uncertain = true;
                break;
            }
            if event.ident != self.pid as usize || event.flags & libc::EV_ERROR != 0 { self.uncertain = true; }
            if event.fflags & libc::NOTE_FORK != 0 { self.uncertain = true; }
            if event.fflags & libc::NOTE_EXIT != 0 { self.exited = true; }
        }
    }

    pub fn stop_and_confirm(&mut self) -> bool {
        self.poll();
        // Reap and this method share the same mutex. An unreaped direct child
        // retains its PID; no PID read from disk or descendant is ever signalled.
        if !self.reaped && !self.exited && unsafe { libc::kill(self.pid as i32, libc::SIGKILL) } < 0 { self.uncertain = true; }
        let deadline = Instant::now() + Duration::from_secs(1);
        while !self.exited && Instant::now() < deadline {
            self.poll();
            if !self.exited { std::thread::sleep(Duration::from_millis(5)); }
        }
        self.exited && !self.uncertain
    }
}

fn checked(code: i32) -> io::Result<()> {
    if code == 0 { Ok(()) } else { Err(io::Error::from_raw_os_error(code)) }
}

struct Attributes(libc::posix_spawnattr_t);
impl Drop for Attributes { fn drop(&mut self) { unsafe { libc::posix_spawnattr_destroy(&mut self.0); } } }
struct Actions(libc::posix_spawn_file_actions_t);
impl Drop for Actions { fn drop(&mut self) { unsafe { libc::posix_spawn_file_actions_destroy(&mut self.0); } } }

fn pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut pair = [0; 2];
    if unsafe { libc::pipe(pair.as_mut_ptr()) } < 0 { return Err(io::Error::last_os_error()); }
    let mut fds = unsafe { (OwnedFd::from_raw_fd(pair[0]), OwnedFd::from_raw_fd(pair[1])) };
    // Keep action sources away from stdin/stdout/stderr, even in a headless host.
    for fd in [&mut fds.0, &mut fds.1] {
        let copy = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
        if copy < 0 { return Err(io::Error::last_os_error()); }
        *fd = unsafe { OwnedFd::from_raw_fd(copy) };
    }
    Ok(fds)
}

pub struct Child {
    pub stdin: Option<std::fs::File>,
    pub stdout: Option<std::fs::File>,
    pub stderr: Option<std::fs::File>,
    pid: u32,
    status: Option<std::process::ExitStatus>,
    tree: Arc<Mutex<KillOnClose>>,
}

impl Child {
    pub fn id(&self) -> u32 { self.pid }
    pub fn resume(&self) -> io::Result<()> {
        let tree = self.tree.lock().map_err(|_| io::Error::other("프로세스 상태 잠금 실패"))?;
        if tree.monitor.reaped { return Err(io::Error::other("이미 종료된 작업입니다")); }
        if unsafe { libc::kill(self.pid as i32, libc::SIGCONT) } < 0 { return Err(io::Error::last_os_error()); }
        Ok(())
    }
    pub fn kill(&mut self) -> io::Result<()> {
        let tree = self.tree.lock().map_err(|_| io::Error::other("프로세스 상태 잠금 실패"))?;
        if self.status.is_some() || tree.monitor.reaped { return Ok(()); }
        if unsafe { libc::kill(self.pid as i32, libc::SIGKILL) } < 0 { return Err(io::Error::last_os_error()); }
        Ok(())
    }
    pub fn try_wait(&mut self) -> io::Result<Option<std::process::ExitStatus>> {
        use std::os::unix::process::ExitStatusExt;
        if self.status.is_some() { return Ok(self.status); }
        let mut tree = self.tree.lock().map_err(|_| io::Error::other("프로세스 상태 잠금 실패"))?;
        let mut status = 0;
        let result = unsafe { libc::waitpid(self.pid as i32, &mut status, libc::WNOHANG) };
        if result < 0 { tree.monitor.uncertain = true; return Err(io::Error::last_os_error()); }
        if result > 0 {
            tree.monitor.reaped = true;
            self.status = Some(std::process::ExitStatus::from_raw(status));
        }
        Ok(self.status)
    }
    pub fn wait(&mut self) -> io::Result<std::process::ExitStatus> {
        self.stdin.take();
        loop {
            if let Some(status) = self.try_wait()? { return Ok(status); }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        if matches!(self.try_wait(), Ok(None)) {
            let _ = self.kill();
            let deadline = Instant::now() + Duration::from_secs(1);
            while matches!(self.try_wait(), Ok(None)) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

/// Native suspended spawn: no provider instruction runs before kqueue is attached.
/// Arguments, environment and cwd go directly to posix_spawn, never a shell.
pub fn spawn(launch: &Launch) -> io::Result<(Child, Arc<Mutex<KillOnClose>>)> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let cwd = if launch.cwd.as_os_str().is_empty() { std::env::current_dir()? } else { launch.cwd.clone() };
    let environment = launch.environment(std::env::vars_os())?;
    let executable = if launch.executable.components().count() > 1 || launch.executable.is_absolute() {
        if launch.executable.is_absolute() { launch.executable.clone() } else { cwd.join(&launch.executable) }
    } else {
        let path = environment.iter().find(|(name, _)| name == "PATH").map(|(_, value)| value)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "실행 파일의 절대 경로나 PATH가 필요합니다"))?;
        std::env::split_paths(path).map(|part| cwd.join(part).join(&launch.executable)).find(|path| path.is_file())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "실행 파일을 찾지 못했습니다"))?
    };
    let executable = CString::new(executable.as_os_str().as_bytes())?;
    let cwd = CString::new(cwd.as_os_str().as_bytes())?;
    let args: Vec<CString> = std::iter::once(launch.executable.as_os_str()).chain(launch.args.iter().map(|a| a.as_os_str()))
        .map(|a| CString::new(a.as_bytes())).collect::<Result<_, _>>()?;
    let env: Vec<CString> = environment.iter().map(|(name, value)| {
        if name.is_empty() || name.as_bytes().contains(&b'=') { return Err(io::Error::other("잘못된 환경 변수 이름입니다")); }
        let mut bytes = name.as_bytes().to_vec(); bytes.push(b'='); bytes.extend_from_slice(value.as_bytes());
        CString::new(bytes).map_err(io::Error::from)
    }).collect::<io::Result<_>>()?;
    let argv: Vec<_> = args.iter().map(|a| a.as_ptr().cast_mut()).chain(std::iter::once(std::ptr::null_mut())).collect();
    let envp: Vec<_> = env.iter().map(|a| a.as_ptr().cast_mut()).chain(std::iter::once(std::ptr::null_mut())).collect();
    let (stdin_read, stdin_write) = pipe()?;
    let (stdout_read, stdout_write) = pipe()?;
    let (stderr_read, stderr_write) = pipe()?;
    let mut attr = std::ptr::null_mut();
    checked(unsafe { libc::posix_spawnattr_init(&mut attr) })?;
    let mut attr = Attributes(attr);
    checked(unsafe { libc::posix_spawnattr_setflags(&mut attr.0, (libc::POSIX_SPAWN_START_SUSPENDED | libc::POSIX_SPAWN_CLOEXEC_DEFAULT) as _) })?;
    let mut actions = std::ptr::null_mut();
    checked(unsafe { libc::posix_spawn_file_actions_init(&mut actions) })?;
    let mut actions = Actions(actions);
    checked(unsafe { libc::posix_spawn_file_actions_addchdir_np(&mut actions.0, cwd.as_ptr()) })?;
    for (fd, target) in [(stdin_read.as_raw_fd(), 0), (stdout_write.as_raw_fd(), 1), (stderr_write.as_raw_fd(), 2)] {
        checked(unsafe { libc::posix_spawn_file_actions_adddup2(&mut actions.0, fd, target) })?;
    }
    for fd in [&stdin_read, &stdin_write, &stdout_read, &stdout_write, &stderr_read, &stderr_write] {
        checked(unsafe { libc::posix_spawn_file_actions_addclose(&mut actions.0, fd.as_raw_fd()) })?;
    }
    let mut pid = 0;
    checked(unsafe { libc::posix_spawn(&mut pid, executable.as_ptr(), &actions.0, &attr.0, argv.as_ptr(), envp.as_ptr()) })?;
    let monitor = match Monitor::new(pid as u32) {
        Ok(monitor) => monitor,
        Err(error) => {
            // No other code owns/reaps this suspended child yet.
            unsafe { libc::kill(pid, libc::SIGKILL); libc::waitpid(pid, std::ptr::null_mut(), 0); }
            return Err(error);
        }
    };
    let tree = Arc::new(Mutex::new(KillOnClose { monitor }));
    Ok((Child { stdin: Some(stdin_write.into()), stdout: Some(stdout_read.into()), stderr: Some(stderr_read.into()), pid: pid as u32, status: None, tree: tree.clone() }, tree))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn arguments_environment_and_pipes_survive_the_start_gate() {
        let mut launch = Launch::new("/usr/bin/printf");
        launch.args(["%s", "quoted ' \" $HOME ; $(false)\n끝"]);
        launch.env_clear();
        let (mut child, tree) = spawn(&launch).unwrap();
        assert!(child.try_wait().unwrap().is_none());
        child.resume().unwrap();
        let mut output = String::new();
        child.stdout.take().unwrap().read_to_string(&mut output).unwrap();
        assert!(child.wait().unwrap().success());
        assert_eq!(output, "quoted ' \" $HOME ; $(false)\n끝");
        assert!(tree.lock().unwrap().stop_and_confirm());
    }

    #[test]
    fn replacement_environment_is_exact() {
        let mut launch = Launch::new("/usr/bin/env");
        launch.env_clear().env("O_MAC_FIXTURE", "value with spaces");
        let (mut child, tree) = spawn(&launch).unwrap();
        child.resume().unwrap();
        let mut output = String::new();
        child.stdout.take().unwrap().read_to_string(&mut output).unwrap();
        assert!(child.wait().unwrap().success());
        assert_eq!(output, "O_MAC_FIXTURE=value with spaces\n");
        assert!(tree.lock().unwrap().stop_and_confirm());
    }

    #[test]
    fn failed_spawn_has_no_child_and_cwd_is_preserved() {
        assert!(spawn(&Launch::new("/no-such-o-test-executable")).is_err());
        let cwd = std::env::temp_dir().canonicalize().unwrap();
        let mut launch = Launch::new("/bin/pwd");
        launch.current_dir(&cwd);
        let (mut child, tree) = spawn(&launch).unwrap();
        child.resume().unwrap();
        let mut output = String::new();
        child.stdout.take().unwrap().read_to_string(&mut output).unwrap();
        assert!(child.wait().unwrap().success());
        assert_eq!(output.trim(), cwd.to_str().unwrap());
        assert!(tree.lock().unwrap().stop_and_confirm());
    }

    #[test]
    fn spawn_fixture() {
        if std::env::var_os("O_MAC_SPAWN_FIXTURE").is_some() {
            assert!(std::process::Command::new("/usr/bin/true").status().unwrap().success());
        }
    }

    #[test]
    fn native_posix_spawn_descendant_is_not_missed() {
        let mut launch = Launch::new(std::env::current_exe().unwrap());
        launch.args(["--exact", "sys::macos_worker::tests::spawn_fixture"]);
        launch.env_clear().env("O_MAC_SPAWN_FIXTURE", "1");
        let (mut child, tree) = spawn(&launch).unwrap();
        child.resume().unwrap();
        assert!(child.wait().unwrap().success());
        assert!(!tree.lock().unwrap().stop_and_confirm());
    }

    #[test]
    fn stop_before_resume_never_executes_provider() {
        let dir = std::env::temp_dir().join(format!("o-mac-before-resume-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("should-not-exist");
        let mut launch = Launch::new("/usr/bin/touch");
        launch.arg(marker.as_os_str());
        let (mut child, tree) = spawn(&launch).unwrap();
        assert!(tree.lock().unwrap().stop_and_confirm());
        assert!(!child.wait().unwrap().success());
        assert!(!marker.exists());
        assert!(tree.lock().unwrap().stop_and_confirm());
    }

    #[test]
    fn fork_is_unknown_even_when_child_was_already_reaped() {
        let mut launch = Launch::new("/bin/sh");
        // Force a fork, including a fully reaped descendant. Darwin gives no
        // complete descendant identities: never silently turn this into proof.
        launch.args(["-c", "/usr/bin/true & wait"]);
        let (mut child, tree) = spawn(&launch).unwrap();
        child.resume().unwrap();
        assert!(child.wait().unwrap().success());
        assert!(!tree.lock().unwrap().stop_and_confirm());
    }
}
