//! Approved execution data travels through a parent-owned pipe, never through run files.

use std::{io::{self, Read, Write}, path::{Path, PathBuf}, process::{Command, Stdio}, sync::mpsc, thread, time::Duration};
use serde::{Deserialize, Serialize};
use super::{mode::RunMode, plan::Plan};

const MAX_INPUT: u64 = 4 * 1024 * 1024;
const READY: &[u8] = b"O-RUN-INPUT-1\n";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunInput {
    pub(crate) home: PathBuf,
    pub(crate) id: String,
    pub(crate) plan: Plan,
    pub(crate) mode: RunMode,
    pub(crate) isolated: bool,
    pub(crate) scheduled: bool,
    pub(crate) followup: Option<super::followup::Input>,
}

impl RunInput {
    pub fn new(home: &Path, id: &str, plan: &Plan, mode: RunMode, isolated: bool, scheduled: bool) -> Self {
        Self { home: home.into(), id: id.into(), plan: plan.clone(), mode, isolated, scheduled, followup: None }
    }
    pub fn id(&self) -> &str { &self.id }
    pub fn plan(&self) -> &Plan { &self.plan }
    pub fn mode(&self) -> RunMode { self.mode }
    pub fn scheduled(&self) -> bool { self.scheduled }
    pub fn is_followup(&self) -> bool { self.followup.is_some() }
}

pub type StartRun<'a> = dyn Fn(&str, &RunInput) -> io::Result<()> + 'a;

/// The acknowledgment keeps the trusted parent alive until the child verifies its identity.
pub fn spawn(command: &mut Command, input: &RunInput) -> io::Result<()> {
    let bytes = serde_json::to_vec(input).map_err(io::Error::other)?;
    if bytes.len() as u64 > MAX_INPUT { return Err(io::Error::other("승인 실행 입력이 너무 큽니다")); }
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    command.stdin(Stdio::piped());
    #[cfg(unix)]
    let mut writer = {
        use std::os::{unix::net::UnixStream, fd::OwnedFd};
        let (writer, reader) = UnixStream::pair()?;
        command.stdin(Stdio::from(OwnedFd::from(reader)));
        writer
    };
    let mut child = crate::sys::proc::spawn_detached_with_stdio(command)?;
    #[cfg(windows)]
    let mut writer = child.stdin.take().ok_or_else(|| io::Error::other("승인 파이프가 없습니다"))?;
    let mut reader = child.stdout.take().ok_or_else(|| io::Error::other("승인 응답 파이프가 없습니다"))?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = writer.write_all(&bytes).and_then(|()| {
            // LOCAL_PEERPID needs the Unix peer connected until the child verifies it.
            #[cfg(unix)]
            writer.shutdown(std::net::Shutdown::Write)?;
            #[cfg(windows)]
            drop(writer);
            let mut ready = [0; READY.len()];
            reader.read_exact(&mut ready)?;
            if ready != READY { return Err(io::Error::other("실행기가 승인 입력을 받지 못했습니다")); }
            Ok(())
        });
        let _ = tx.send(result);
    });
    match rx.recv_timeout(Duration::from_secs(15)) {
        Ok(Ok(())) => Ok(()),
        result => {
            let _ = child.kill();
            let _ = child.wait();
            Err(match result { Ok(Err(error)) => error, _ => io::Error::other("실행기의 승인 입력 확인 시간이 지났습니다") })
        }
    }
}

/// Direct internal-command calls, files and ordinary shell pipes are not an approval source.
pub fn receive(home: &Path, id: &str, followup: bool) -> Result<RunInput, String> {
    verify_parent().map_err(|e| format!("승인한 부모 실행기의 파이프가 필요합니다: {e}"))?;
    let mut bytes = Vec::new();
    io::stdin().take(MAX_INPUT + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_INPUT { return Err("승인 실행 입력이 너무 큽니다".into()); }
    let input: RunInput = serde_json::from_slice(&bytes).map_err(|_| "승인 실행 입력이 없거나 올바르지 않습니다")?;
    if input.id != id || !super::store::is_run_id(id) || !crate::projects::same(&input.home.to_string_lossy(), &home.to_string_lossy()) || input.is_followup() != followup || (input.scheduled && input.mode.yolo) {
        return Err("승인 실행 입력이 요청한 실행과 다릅니다".into());
    }
    io::stdout().write_all(READY).and_then(|()| io::stdout().flush()).map_err(|e| e.to_string())?;
    Ok(input)
}

fn trusted_parent(image: &Path) -> io::Result<()> {
    let current = std::env::current_exe()?.canonicalize()?;
    let image = image.canonicalize()?;
    if image == current { return Ok(()); }
    Err(io::Error::other("같은 실행 파일의 부모가 전달한 입력이 아닙니다"))
}

#[cfg(windows)]
fn verify_parent() -> io::Result<()> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::{Pipes::GetNamedPipeServerProcessId, Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION}};
    let mut pid = 0;
    if unsafe { GetNamedPipeServerProcessId(io::stdin().as_raw_handle(), &mut pid) } == 0 || pid == std::process::id() { return Err(io::Error::other("부모가 만든 파이프가 아닙니다")); }
    let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if raw.is_null() { return Err(io::Error::last_os_error()); }
    let process = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut image = vec![0u16; 32768];
    let mut length = image.len() as u32;
    if unsafe { QueryFullProcessImageNameW(process.as_raw_handle(), 0, image.as_mut_ptr(), &mut length) } == 0 { return Err(io::Error::last_os_error()); }
    use std::os::windows::ffi::OsStringExt;
    trusted_parent(Path::new(&std::ffi::OsString::from_wide(&image[..length as usize])))
}

#[cfg(target_os = "linux")]
fn verify_parent() -> io::Result<()> {
    let mut peer: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of_val(&peer) as libc::socklen_t;
    if unsafe { libc::getsockopt(0, libc::SOL_SOCKET, libc::SO_PEERCRED, (&mut peer as *mut libc::ucred).cast(), &mut length) } != 0 || peer.pid <= 0 || peer.pid == std::process::id() as i32 { return Err(io::Error::other("부모가 만든 소켓이 아닙니다")); }
    trusted_parent(&std::fs::read_link(format!("/proc/{}/exe", peer.pid))?)
}

#[cfg(target_os = "macos")]
fn verify_parent() -> io::Result<()> {
    let mut pid: libc::pid_t = 0;
    let mut length = std::mem::size_of_val(&pid) as libc::socklen_t;
    if unsafe { libc::getsockopt(0, libc::SOL_LOCAL, libc::LOCAL_PEERPID, (&mut pid as *mut libc::pid_t).cast(), &mut length) } != 0 || pid <= 0 || pid == std::process::id() as i32 { return Err(io::Error::other("부모가 만든 소켓이 아닙니다")); }
    let mut bytes = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let count = unsafe { libc::proc_pidpath(pid, bytes.as_mut_ptr().cast(), bytes.len() as u32) };
    if count <= 0 { return Err(io::Error::last_os_error()); }
    use std::os::unix::ffi::OsStrExt;
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(count as usize);
    trusted_parent(Path::new(std::ffi::OsStr::from_bytes(&bytes[..end])))
}
