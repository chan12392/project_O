//! A launchd-owned supervisor gives each worker a dedicated kernel coalition.
//! Credentials travel only over private, peer-checked local sockets.
use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{PermissionsExt, MetadataExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::{ExitStatusExt, CommandExt};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};
use super::{launch::Launch, macos_coalition::Coalition, macos_native, proc::KillOnClose};

#[derive(Serialize, Deserialize)]
struct Request { executable: Vec<u8>, args: Vec<Vec<u8>>, cwd: Vec<u8>, environment: Vec<(Vec<u8>, Vec<u8>)> }
#[derive(Serialize, Deserialize)]
struct Ready { pid: u32, coalition: u64 }
#[derive(Default, Serialize, Deserialize)]
struct Finished { status: Option<i32>, confirmed: bool }
struct Shared { result: Mutex<Option<Finished>>, control: Mutex<UnixStream> }

fn send<T: Serialize>(stream: &mut UnixStream, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    let size = u32::try_from(bytes.len()).map_err(io::Error::other)?;
    stream.write_all(&size.to_be_bytes())?; stream.write_all(&bytes)
}
fn receive<T: serde::de::DeserializeOwned>(stream: &mut UnixStream) -> io::Result<T> {
    let mut size = [0; 4]; stream.read_exact(&mut size)?;
    let size = u32::from_be_bytes(size) as usize;
    if size > 16 * 1024 * 1024 { return Err(io::Error::other("작업 시작 메시지가 너무 큽니다")); }
    let mut bytes = vec![0; size]; stream.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}
fn peer(stream: &UnixStream) -> io::Result<u32> {
    let mut uid = 0; let mut gid = 0;
    let mut pid = 0i32; let mut len = 4;
    if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0 || uid != unsafe { libc::geteuid() } ||
        unsafe { libc::getsockopt(stream.as_raw_fd(), 0, 2, (&mut pid as *mut i32).cast(), &mut len) } != 0 || len != 4 || pid <= 0 {
        return Err(io::Error::other("macOS 실행 도우미의 신원을 확인하지 못했습니다"));
    }
    Ok(pid as u32)
}
fn launchctl(args: &[&OsStr]) -> io::Result<std::process::Output> {
    let output = Command::new("/bin/launchctl").args(args).output()?;
    if !output.status.success() { return Err(io::Error::other(format!("macOS 실행 묶음 시작 실패: {}", String::from_utf8_lossy(&output.stderr)))); }
    Ok(output)
}
fn xml(value: &str) -> String { value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;") }

struct Job { directory: PathBuf, target: String, loaded: bool }
impl Drop for Job {
    fn drop(&mut self) {
        if self.loaded { let _ = Command::new("/bin/launchctl").args(["bootout", &self.target]).output(); }
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

pub struct Monitor { shared: Arc<Shared>, coalition: Coalition, _job: Job }
impl Monitor {
    pub fn stop_and_confirm(&mut self) -> bool {
        let _ = self.shared.control.lock().map(|mut control| control.write_all(b"S"));
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            if let Ok(result) = self.shared.result.lock() {
                if let Some(result) = result.as_ref() {
                    // The supervisor stays alive until this handle is closed.
                    return result.confirmed && self.coalition.active().ok() == Some(1);
                }
            } else { return false; }
            if Instant::now() >= deadline { return false; }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        let _ = self.stop_and_confirm();
        // Reader thread owns another descriptor, so explicitly close the write
        // side. The helper cleans up even when the original owner crashes.
        if let Ok(control) = self.shared.control.lock() { let _ = control.shutdown(std::net::Shutdown::Write); }
    }
}

pub struct Child {
    pub stdin: Option<UnixStream>, pub stdout: Option<UnixStream>, pub stderr: Option<UnixStream>,
    pid: u32, shared: Arc<Shared>, tree: Arc<Mutex<KillOnClose>>,
}
impl Child {
    pub fn id(&self) -> u32 { self.pid }
    pub fn resume(&self) -> io::Result<()> { self.shared.control.lock().map_err(|_| io::Error::other("작업 잠금 실패"))?.write_all(b"R") }
    pub fn kill(&mut self) -> io::Result<()> { self.shared.control.lock().map_err(|_| io::Error::other("작업 잠금 실패"))?.write_all(b"S") }
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let result = self.shared.result.lock().map_err(|_| io::Error::other("작업 잠금 실패"))?;
        match result.as_ref() {
            Some(Finished { status: Some(status), .. }) => Ok(Some(ExitStatus::from_raw(*status))),
            Some(_) => Err(io::Error::other("macOS 실행 도우미가 종료 상태를 확인하지 못했습니다")),
            None => Ok(None),
        }
    }
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        self.stdin.take();
        loop { if let Some(status) = self.try_wait()? { return Ok(status); } std::thread::sleep(Duration::from_millis(10)); }
    }
}
impl Drop for Child { fn drop(&mut self) { if let Ok(mut tree) = self.tree.lock() { tree.stop_and_confirm(); } } }

fn helper_path() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let parent = exe.parent().ok_or_else(|| io::Error::other("실행 파일 폴더 없음"))?;
    if exe.file_name() == Some(OsStr::new("o")) || exe.file_name() == Some(OsStr::new("o-handle-owner")) { return Ok(exe); }
    // Cargo's integration harnesses live in deps; their explicitly built
    // fixture lives next to deps. Installed GUI uses its bundled o sidecar.
    let helper = if parent.file_name() == Some(OsStr::new("deps")) { parent.parent().unwrap().join("o-handle-owner") } else { parent.join("o") };
    if !helper.is_file() { return Err(io::Error::other("macOS 실행 도우미 o를 찾지 못했습니다")); }
    Ok(helper)
}

pub fn spawn(launch: &Launch) -> io::Result<(Child, Arc<Mutex<KillOnClose>>)> {
    let helper = helper_path()?;
    let mut nonce = [0u8; 12]; unsafe { libc::arc4random_buf(nonce.as_mut_ptr().cast(), nonce.len()); }
    let suffix: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
    let directory = PathBuf::from(format!("/private/tmp/o-worker-{suffix}"));
    // No credentials or provider arguments are written here.
    std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
    let domain = format!("gui/{}", unsafe { libc::geteuid() });
    let label = format!("local.project-o.worker.{suffix}");
    let mut job = Job { directory, target: format!("{domain}/{label}"), loaded: false };
    let socket = job.directory.join("ipc");
    let listener = UnixListener::bind(&socket)?; listener.set_nonblocking(true)?;
    let plist = job.directory.join("job.plist");
    let helper = helper.to_str().ok_or_else(|| io::Error::other("도우미 경로 인코딩 오류"))?;
    std::fs::write(&plist, format!(r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>Label</key><string>{label}</string><key>ProgramArguments</key><array><string>{}</string><string>--o-mac-worker</string><string>{}</string></array><key>KeepAlive</key><false/><key>RunAtLoad</key><false/><key>ProcessType</key><string>Background</string></dict></plist>"#, xml(helper), xml(socket.to_str().unwrap())))?;
    launchctl(&[OsStr::new("bootstrap"), OsStr::new(&domain), plist.as_os_str()])?; job.loaded = true;
    let output = launchctl(&[OsStr::new("kickstart"), OsStr::new("-p"), OsStr::new(&job.target)])?;
    let helper_pid: u32 = String::from_utf8_lossy(&output.stdout).trim().parse().map_err(io::Error::other)?;
    let coalition = Coalition::isolated(helper_pid, std::process::id())?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut sockets = Vec::new();
    while sockets.len() < 4 {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false)?;
                if peer(&stream)? != helper_pid { return Err(io::Error::other("예상하지 않은 실행 도우미")); }
                stream.set_read_timeout(Some(Duration::from_secs(10)))?;
                stream.set_write_timeout(Some(Duration::from_secs(10)))?;
                sockets.push(stream);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            Err(error) => return Err(error),
        }
    }
    let mut iter = sockets.into_iter();
    let mut control = iter.next().unwrap(); let stdin = iter.next().unwrap(); let stdout = iter.next().unwrap(); let stderr = iter.next().unwrap();
    let cwd = if launch.cwd.as_os_str().is_empty() { std::env::current_dir()? } else { launch.cwd.clone() };
    let request = Request { executable: launch.executable.as_os_str().as_bytes().to_vec(), args: launch.args.iter().map(|a| a.as_bytes().to_vec()).collect(), cwd: cwd.as_os_str().as_bytes().to_vec(), environment: launch.environment(std::env::vars_os())?.into_iter().map(|(k,v)| (k.as_bytes().to_vec(),v.as_bytes().to_vec())).collect() };
    send(&mut control, &request)?;
    let ready: Ready = receive(&mut control)?;
    if ready.coalition != coalition.id || super::macos_coalition::id(ready.pid)? != coalition.id { return Err(io::Error::other("작업의 실행 묶음이 일치하지 않습니다")); }
    for stream in [&control, &stdin, &stdout, &stderr] { stream.set_read_timeout(None)?; stream.set_write_timeout(None)?; }
    let mut reader = control.try_clone()?;
    let shared = Arc::new(Shared { result: Mutex::new(None), control: Mutex::new(control) });
    let read_shared = shared.clone();
    std::thread::spawn(move || {
        let result: Finished = receive(&mut reader).unwrap_or_default();
        if let Ok(mut stored) = read_shared.result.lock() { *stored = Some(result); }
    });
    let tree = Arc::new(Mutex::new(KillOnClose { monitor: Monitor { shared: shared.clone(), coalition, _job: job } }));
    Ok((Child { stdin: Some(stdin), stdout: Some(stdout), stderr: Some(stderr), pid: ready.pid, shared, tree: tree.clone() }, tree))
}

use std::os::unix::fs::DirBuilderExt;

/// Only the bundled CLI/test fixture invokes this entry point, before CLI hooks.
pub fn run_helper(args: &[OsString]) -> io::Result<()> {
    if args.len() != 1 { return Err(io::Error::other("실행 도우미 인자 오류")); }
    let socket = Path::new(&args[0]);
    let directory = socket.parent().ok_or_else(|| io::Error::other("소켓 폴더 없음"))?;
    let suffix = directory.file_name().and_then(OsStr::to_str).and_then(|s| s.strip_prefix("o-worker-"))
        .filter(|s| s.len() == 24 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| io::Error::other("실행 폴더 이름 오류"))?;
    if directory.parent() != Some(Path::new("/private/tmp")) { return Err(io::Error::other("실행 폴더 위치 오류")); }
    let metadata = std::fs::symlink_metadata(directory)?;
    if !metadata.is_dir() || metadata.uid() != unsafe { libc::geteuid() } || metadata.permissions().mode() & 0o077 != 0 { return Err(io::Error::other("비공개 실행 폴더가 아닙니다")); }
    let mut control = UnixStream::connect(socket)?;
    let parent = peer(&control)?;
    let coalition = Coalition::isolated(std::process::id(), parent)?;
    let mut stdin = UnixStream::connect(socket)?; let mut stdout = UnixStream::connect(socket)?; let mut stderr = UnixStream::connect(socket)?;
    for stream in [&control, &stdin, &stdout, &stderr] { if peer(stream)? != parent { return Err(io::Error::other("실행 소켓 소유자가 바뀌었습니다")); } }
    control.set_read_timeout(Some(Duration::from_secs(10)))?;
    let request: Request = receive(&mut control)?;
    let mut launch = Launch::new(OsString::from_vec(request.executable));
    launch.args(request.args.into_iter().map(OsString::from_vec)); launch.current_dir(OsString::from_vec(request.cwd));
    launch.env_clear().envs(request.environment.into_iter().map(|(k,v)| (OsString::from_vec(k), OsString::from_vec(v))));
    let _cleanup = super::macos_coalition::Cleanup(&coalition);
    let (mut child, _native_tree) = macos_native::spawn(&launch)?;
    let child_pid = child.id();
    if super::macos_coalition::id(child_pid)? != coalition.id { return Err(io::Error::other("작업 묶음 상속 실패")); }
    send(&mut control, &Ready { pid: child_pid, coalition: coalition.id })?;
    let mut input = child.stdin.take().unwrap();
    std::thread::spawn(move || { let _ = io::copy(&mut stdin, &mut input); });
    let mut output = child.stdout.take().unwrap();
    std::thread::spawn(move || { let _ = io::copy(&mut output, &mut stdout); });
    let mut errors = child.stderr.take().unwrap();
    std::thread::spawn(move || { let _ = io::copy(&mut errors, &mut stderr); });
    control.set_read_timeout(Some(Duration::from_millis(20)))?;
    let mut stopped = false; let mut disconnected = false; let mut status = None; let mut deadline = None;
    loop {
        let mut command = [0];
        match control.read(&mut command) {
            Ok(0) => { stopped = true; disconnected = true; }
            Ok(_) if command[0] == b'R' && !stopped => { if child.resume().is_err() { stopped = true; } }
            Ok(_) if command[0] == b'S' => stopped = true,
            Ok(_) => {},
            Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted) => {},
            Err(_) => { stopped = true; disconnected = true; },
        }
        if status.is_none() { match child.try_wait() { Ok(Some(value)) => { status = Some(value.into_raw()); stopped = true; }, Ok(None) => {}, Err(_) => stopped = true } }
        if stopped {
            let until = *deadline.get_or_insert_with(|| Instant::now() + Duration::from_secs(5));
            let _ = coalition.stop_members();
            // Zombie children count as exited, but still reap our direct child.
            if status.is_none() { status = child.try_wait()?.map(ExitStatus::into_raw); }
            if coalition.active().ok() == Some(1) && status.is_some() { break; }
            if Instant::now() >= until {
                let _ = send(&mut control, &Finished { status, confirmed: false });
                return Err(io::Error::other("하위 작업 종료 확인 시간 초과"));
            }
        }
    }
    // Pipe relays close independently after draining. Never block exit proof
    // on a caller that has stopped reading a full output pipe.
    if !disconnected {
        send(&mut control, &Finished { status, confirmed: true })?;
        // Keep coalition accounting alive for the parent to independently check.
        loop {
            let mut byte = [0];
            match control.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => {},
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted) => {},
                Err(_) => break,
            }
        }
    }
    drop(child); drop(_native_tree); drop(_cleanup);
    // Retire even after the parent was killed. No persistent LaunchAgent or
    // completed launchd entry is left by a successful emergency cleanup.
    let _ = std::fs::remove_dir_all(directory);
    let target = format!("gui/{}/local.project-o.worker.{suffix}", unsafe { libc::geteuid() });
    Err(Command::new("/bin/launchctl").args(["bootout", &target]).exec())
}
