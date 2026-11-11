//! Terminal sessions under `<home>/terminals/<id>/`.
//! `session.json` is metadata, `host.json` identifies the host process, `exit.json`
//! records its exit, `starting.json` tracks a detached host before it is ready,
//! `output/` holds offset-named raw chunks, and `requests/` holds
//! atomic input, resize, paste approval, and close requests for the host.

use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct Stability(Option<(u64, std::time::SystemTime, Instant)>);
impl Stability {
    pub(crate) fn ready(&mut self, metadata: &fs::Metadata, duration: Duration) -> bool {
        let Ok(modified) = metadata.modified() else { self.0 = None; return false };
        let size = metadata.len();
        match self.0 {
            Some((old_size, old_modified, since)) if size == old_size && modified == old_modified => since.elapsed() >= duration,
            _ => { self.0 = Some((size, modified, Instant::now())); false }
        }
    }
}

use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use crate::sys::{js, launch::Launch, proc, resolve};

#[cfg(any(windows, test))]
const MAX_OUTPUT: u64 = 8 * 1024 * 1024;
/// The most one write hands the terminal; the host drops anything larger left in its folders.
const MAX_INPUT: usize = 64 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub provider: String,
    pub project: String,
    pub purpose: String,
    pub thread_id: Option<String>,
    pub run_id: Option<String>,
    pub card_id: Option<String>,
    pub state: String,
    pub exit_code: Option<i64>,
    pub cols: u16,
    pub rows: u16,
    pub started_at: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenRequest {
    pub provider: String,
    pub project: String,
    pub thread: Option<String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chunk { pub offset: u64, pub data: String, pub state: String, pub exit_code: Option<i64>, pub start: u64 }

#[derive(Serialize, Deserialize)]
struct Host { pid: u32, started: u64, #[serde(default, skip_serializing_if = "Option::is_none")] worker: Option<u32>, #[serde(default, skip_serializing_if = "Option::is_none")] job: Option<String> }

pub fn is_session_id(id: &str) -> bool {
    let b = id.as_bytes();
    b.len() == 22 && &b[..2] == b"t-" && b[2..10].iter().all(u8::is_ascii_digit) && b[10] == b'-' && b[11..17].iter().all(u8::is_ascii_digit) && b[17] == b'-' && b[18..].iter().all(u8::is_ascii_alphanumeric)
}

fn dir(home: &Path, id: &str) -> io::Result<PathBuf> {
    if !is_session_id(id) { return Err(io::Error::new(io::ErrorKind::InvalidInput, "잘못된 터미널 세션 ID")); }
    Ok(home.join("terminals").join(id))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> io::Result<T> {
    serde_json::from_slice(&fs::read(path)?).map_err(io::Error::other)
}

fn save_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    let temp = path.with_extension(format!("tmp-{}", crate::run::store::new_run_id(Utc::now())));
    fs::write(&temp, bytes)?;
    fs::rename(temp, path)
}

fn create(home: &Path, mut session: Session) -> io::Result<Session> {
    fs::create_dir_all(home.join("terminals"))?;
    for _ in 0..100 {
        session.id = format!("t-{}", crate::run::store::new_run_id(Utc::now()));
        let folder = dir(home, &session.id)?;
        match fs::create_dir(&folder) {
            Ok(()) => {
                fs::create_dir(folder.join("requests"))?;
                fs::create_dir(folder.join("output"))?;
                fs::create_dir(folder.join("inputs"))?;
                save_json(&folder.join("session.json"), &session)?;
                return Ok(session);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "터미널 ID를 만들지 못했습니다"))
}

fn dimensions(cols: Option<u16>, rows: Option<u16>) -> io::Result<(u16, u16)> {
    let (cols, rows) = (cols.unwrap_or(120), rows.unwrap_or(32));
    if cols == 0 || rows == 0 || cols > i16::MAX as u16 || rows > i16::MAX as u16 { return Err(io::Error::new(io::ErrorKind::InvalidInput, "잘못된 터미널 크기")); }
    Ok((cols, rows))
}

fn session(provider: String, project: String, purpose: &str, cols: u16, rows: u16) -> Session {
    Session { id: String::new(), provider, project, purpose: purpose.into(), thread_id: None, run_id: None, card_id: None, state: "running".into(), exit_code: None, cols, rows, started_at: js::now_iso() }
}

fn lead_launch(session: &Session) -> io::Result<Launch> {
    let cli = resolve::resolve_cli(&session.provider).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "설치된 에이전트 CLI를 찾지 못했습니다"))?;
    let mut launch = Launch::new(cli.command);
    launch.args(cli.prefix).current_dir(&session.project).env("O_AGENT_ROLE", "lead");
    Ok(launch)
}

/// Opens a human lead using the O executable supplied by the caller.
pub fn open_with_host(home: &Path, req: OpenRequest, host_exe: &Path) -> io::Result<Session> {
    if !cfg!(windows) { return Err(io::Error::new(io::ErrorKind::Unsupported, "이 운영체제에서는 터미널을 지원하지 않습니다")); }
    if !matches!(req.provider.as_str(), "claude" | "codex" | "agy" | "opencode" | "devin") { return Err(io::Error::new(io::ErrorKind::InvalidInput, "알 수 없는 에이전트")); }
    let project = Path::new(&req.project);
    if !project.is_absolute() || !project.is_dir() { return Err(io::Error::new(io::ErrorKind::InvalidInput, "존재하는 절대 프로젝트 경로가 필요합니다")); }
    crate::lead::check_folder(home, &req.project).map_err(io::Error::other)?;
    if let Some(thread) = req.thread.as_deref() { crate::lead::attachment_thread(home, &req.project, Some(thread)).map_err(io::Error::other)?; }
    let (cols, rows) = dimensions(req.cols, req.rows)?;
    let mut s = session(req.provider, req.project, "lead", cols, rows);
    s.thread_id = req.thread;
    let _ = lead_launch(&s)?;
    let s = create(home, s)?;
    let mut child = match proc::spawn_detached(Command::new(host_exe).args(["_term", &s.id])) {
        Ok(child) => child,
        Err(error) => { let _ = save_json(&dir(home, &s.id)?.join("exit.json"), &None::<i64>); return Err(error); }
    };
    let started = match proc::started_at(child.id()) {
        Some(started) => started,
        None => { let _ = child.kill(); let _ = child.wait(); let _ = save_json(&dir(home, &s.id)?.join("exit.json"), &None::<i64>); return Err(io::Error::other("호스트 프로세스 식별 실패")); }
    };
    if let Err(error) = save_json(&dir(home, &s.id)?.join("starting.json"), &Host { pid: child.id(), started, worker: None, job: None }) {
        let _ = child.kill(); let _ = child.wait(); let _ = save_json(&dir(home, &s.id)?.join("exit.json"), &None::<i64>); return Err(error);
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let folder = dir(home, &s.id)?;
        if folder.join("exit.json").exists() { return Err(io::Error::other("터미널 호스트를 시작하지 못했습니다")); }
        if folder.join("host.json").exists() { return get(home, &s.id); }
        if proc::definitely_exited(child.id(), started) { return Err(io::Error::other("터미널 호스트가 시작 중 종료됐습니다")); }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = save_json(&dir(home, &s.id)?.join("exit.json"), &None::<i64>);
    Err(io::Error::new(io::ErrorKind::TimedOut, "터미널 호스트를 기다리는 시간이 지났습니다"))
}

pub fn open(home: &Path, req: OpenRequest) -> io::Result<Session> {
    let exe = std::env::current_exe()?;
    let name = if cfg!(windows) { "o.exe" } else { "o" };
    let host = if exe.file_name().is_some_and(|s| s.eq_ignore_ascii_case(name)) { Some(exe.clone()) } else { exe.parent().map(|p| p.join(name)).filter(|p| p.is_file()).or_else(|| resolve::which("o")) };
    open_with_host(home, req, &host.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "O CLI를 찾지 못했습니다"))?)
}

pub fn get(home: &Path, id: &str) -> io::Result<Session> {
    let folder = dir(home, id)?;
    let mut s: Session = read_json(&folder.join("session.json"))?;
    if let Ok(code) = read_json::<Option<i64>>(&folder.join("exit.json")) { s.state = "exited".into(); s.exit_code = code; }
    else if let Ok(host) = read_json::<Host>(&folder.join("host.json")).or_else(|_| read_json::<Host>(&folder.join("starting.json"))) {
        if proc::definitely_exited(host.pid, host.started) { s.state = "exited".into(); }
    }
    Ok(s)
}

pub fn list(home: &Path) -> io::Result<Vec<Session>> {
    let entries = match fs::read_dir(home.join("terminals")) { Ok(e) => e, Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()), Err(e) => return Err(e) };
    let mut sessions: Vec<_> = entries.flatten().filter_map(|e| e.file_name().to_str().and_then(|id| get(home, id).ok())).collect();
    sessions.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(sessions)
}

fn output_files(folder: &Path) -> io::Result<Vec<(u64, PathBuf)>> {
    let mut files: Vec<_> = fs::read_dir(folder.join("output"))?.flatten().filter_map(|e| e.file_name().to_str()?.parse::<u64>().ok().map(|offset| (offset, e.path()))).collect();
    files.sort_by_key(|(offset, _)| *offset);
    Ok(files)
}

#[cfg(any(windows, test))]
fn trim_output(folder: &Path, end: u64) -> io::Result<()> {
    for (at, path) in output_files(folder)? {
        if at + fs::metadata(&path)?.len() <= end.saturating_sub(MAX_OUTPUT) { fs::remove_file(path)?; }
    }
    Ok(())
}

pub fn read(home: &Path, id: &str, offset: u64) -> io::Result<Chunk> {
    let folder = dir(home, id)?;
    let s = get(home, id)?;
    for _ in 0..3 {
        let files = match output_files(&folder) { Ok(files) => files, Err(_) => continue };
        let start = files.first().map_or(0, |(at, _)| *at);
        let mut next = offset.max(start);
        let mut data = Vec::new();
        let mut trimmed = false;
        for (at, path) in files {
            let bytes = match fs::read(path) { Ok(b) => b, Err(_) => { trimmed = true; break; } };
            let end = at + bytes.len() as u64;
            if end > next && at <= next { data.extend_from_slice(&bytes[(next - at) as usize..]); next = end; }
        }
        if !trimmed { return Ok(Chunk { offset: next, data: STANDARD.encode(data), state: s.state, exit_code: s.exit_code, start }); }
    }
    Err(io::Error::new(io::ErrorKind::Interrupted, "터미널 출력을 갱신 중입니다. 다시 읽어 주세요"))
}

fn request(home: &Path, id: &str, kind: &str, bytes: &[u8]) -> io::Result<()> {
    if get(home, id)?.state == "exited" { return Err(io::Error::other("종료된 터미널입니다")); }
    let requests = dir(home, id)?.join("requests");
    let name = format!("{:020}-{}-{kind}", Utc::now().timestamp_nanos_opt().unwrap_or_default(), crate::run::store::new_run_id(Utc::now()));
    let tmp = requests.join(format!(".{name}"));
    let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    f.write_all(bytes)?;
    drop(f);
    fs::rename(tmp, requests.join(name))
}

pub fn write(home: &Path, id: &str, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > MAX_INPUT { return Err(io::Error::new(io::ErrorKind::InvalidInput, "터미널 입력은 한 번에 64 KiB까지 가능합니다")); }
    if get(home, id)?.state == "exited" { return Err(io::Error::other("종료된 터미널입니다")); }
    if bytes.is_empty() { return Ok(()); }
    let mut queue = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dir(home, id)?.join("input.queue"))?;
    queue.lock()?;
    let mut counter = [0u8; 8];
    if queue.metadata()?.len() != 0 { queue.read_exact(&mut counter)?; }
    let sequence = u64::from_le_bytes(counter);
    let next = sequence.checked_add(1).ok_or_else(|| io::Error::other("터미널 입력 순서가 가득 찼습니다"))?;
    queue.seek(SeekFrom::Start(0))?;
    queue.write_all(&next.to_le_bytes())?;
    let inputs = dir(home, id)?.join("inputs");
    let name = format!("{sequence:020}");
    let tmp = inputs.join(format!(".{name}"));
    let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    file.write_all(bytes)?;
    drop(file);
    fs::rename(tmp, inputs.join(name))
}
pub fn decode_data(data: &str) -> io::Result<Vec<u8>> { STANDARD.decode(data).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "잘못된 base64 입력")) }
pub fn resize(home: &Path, id: &str, cols: u16, rows: u16) -> io::Result<()> { dimensions(Some(cols), Some(rows))?; request(home, id, "resize", &serde_json::to_vec(&(cols, rows)).map_err(io::Error::other)?) }
pub fn paste(home: &Path, id: &str) -> io::Result<()> { if get(home, id)?.state != "awaiting-paste" { return Err(io::Error::other("승인할 붙여넣기가 없습니다")); } request(home, id, "paste", &[]) }
pub fn close(home: &Path, id: &str) -> io::Result<()> { request(home, id, "close", &[]) }

/// Runs a detached lead host; callers must supply the real O home.
pub fn host(home: &Path, id: &str) -> io::Result<()> {
    let s = get(home, id)?;
    if s.purpose != "lead" { return Err(io::Error::other("총괄 터미널이 아닙니다")); }
    let _lock = OpenOptions::new().write(true).create_new(true).open(dir(home, id)?.join("host.lock"))?;
    let launch = match lead_launch(&s) { Ok(launch) => launch, Err(error) => { let _ = save_json(&dir(home, id)?.join("exit.json"), &None::<i64>); return Err(error); } };
    let watcher = s.thread_id.as_ref().map(|thread| {
        let (send, receive) = std::sync::mpsc::channel();
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let (home, project, thread, folder) = (home.to_path_buf(), s.project.clone(), thread.clone(), dir(home, id).unwrap());
        let worker = std::thread::spawn(move || {
            let mut stability = std::collections::HashMap::new();
            loop {
                let _ = crate::lead::pickup(&home, &project, &thread, &folder, &mut stability, &worker_cancelled);
                if receive.recv_timeout(Duration::from_millis(250)).is_ok() { break; }
            }
        });
        (send, worker, cancelled)
    });
    let result = run_host(home, s, launch, None, None, #[cfg(test)] Duration::ZERO);
    if let Some((send, worker, cancelled)) = watcher { cancelled.store(true, std::sync::atomic::Ordering::Release); let _ = send.send(()); let _ = worker.join(); }
    result
}

/// Starts a card in the runner process; the launch environment stays in memory.
#[allow(clippy::too_many_arguments)]
pub fn open_card(home: &Path, provider: String, project: String, run_id: String, card_id: String, launch: Launch, line: String, confirm: bool) -> io::Result<Session> {
    open_card_tracked(home, provider, project, run_id, card_id, launch, line, confirm, std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)), std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)), #[cfg(test)] (Duration::from_secs(10), Duration::ZERO))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn open_card_tracked(home: &Path, provider: String, project: String, run_id: String, card_id: String, launch: Launch, line: String, confirm: bool, no_process: std::sync::Arc<std::sync::atomic::AtomicBool>, exited: std::sync::Arc<std::sync::atomic::AtomicBool>, #[cfg(test)] timing: (Duration, Duration)) -> io::Result<Session> {
    if !cfg!(windows) { return Err(io::Error::new(io::ErrorKind::Unsupported, "이 운영체제에서는 터미널을 지원하지 않습니다")); }
    let mut s = session(provider, project, "card", 120, 32);
    s.run_id = Some(run_id); s.card_id = Some(card_id);
    if confirm { s.state = "awaiting-paste".into(); }
    let s = create(home, s)?;
    let home = home.to_owned();
    let hosted = s.clone();
    let host_home = home.clone();
    no_process.store(false, std::sync::atomic::Ordering::Release);
    std::thread::spawn(move || { let _ = run_host(&host_home, hosted, launch, Some((line, confirm)), Some(no_process), #[cfg(test)] timing.1); });
    let folder = dir(&home, &s.id)?;
    #[cfg(not(test))]
    let deadline = Instant::now() + Duration::from_secs(10);
    #[cfg(test)]
    let deadline = Instant::now() + timing.0;
    while !folder.join("host.json").exists() {
        if folder.join("exit.json").exists() {
            exited.store(folder.join("tree-exited").is_file(), std::sync::atomic::Ordering::Release);
            return Err(io::Error::other("터미널 프로세스를 시작하지 못했습니다"));
        }
        if Instant::now() >= deadline {
            let _ = close(&home, &s.id);
            let _ = wait(&home, &s.id, Duration::from_secs(10));
            exited.store(folder.join("tree-exited").is_file(), std::sync::atomic::Ordering::Release);
            return Err(io::Error::new(io::ErrorKind::TimedOut, "터미널 시작 대기 시간이 지났습니다"));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(s)
}

#[cfg(windows)]
pub(crate) fn card_worker(home: &Path, id: &str) -> io::Result<(u32, Option<String>)> {
    let host: Host = read_json(&dir(home, id)?.join("host.json"))?;
    Ok((host.worker.ok_or_else(|| io::Error::other("터미널 프로세스 ID가 없습니다"))?, host.job))
}

#[cfg(windows)]
pub(crate) fn tree_confirmed(home: &Path, id: &str) -> bool { dir(home, id).is_ok_and(|folder| folder.join("tree-exited").is_file()) }
#[cfg(windows)]
pub(crate) fn pasted(home: &Path, id: &str) -> bool { dir(home, id).is_ok_and(|folder| folder.join("pasted").is_file()) }

pub fn wait(home: &Path, id: &str, timeout: Duration) -> io::Result<Session> {
    let deadline = Instant::now() + timeout;
    loop {
        let s = get(home, id)?;
        if s.state == "exited" { return Ok(s); }
        if Instant::now() >= deadline { return Err(io::Error::new(io::ErrorKind::TimedOut, "터미널 종료 대기 시간이 지났습니다")); }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// What a queued input or request file holds. A file over the write limit or anything that is
/// not a plain file is removed unread; one that cannot be read yet stays for the next pass.
#[cfg(windows)]
fn queued_bytes(entry: &fs::DirEntry) -> Option<Vec<u8>> {
    let plain = fs::symlink_metadata(entry.path()).is_ok_and(|meta| meta.is_file() && meta.len() <= MAX_INPUT as u64);
    if !plain {
        let _ = fs::remove_file(entry.path()).or_else(|_| fs::remove_dir(entry.path()));
        return None;
    }
    fs::read(entry.path()).ok().filter(|bytes| bytes.len() <= MAX_INPUT)
}

#[cfg(not(windows))]
fn run_host(_: &Path, _: Session, _: Launch, _: Option<(String, bool)>, no_process: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>, #[cfg(test)] _: Duration) -> io::Result<()> {
    if let Some(flag) = no_process { flag.store(true, std::sync::atomic::Ordering::Release); }
    Err(io::Error::new(io::ErrorKind::Unsupported, "이 운영체제에서는 터미널을 지원하지 않습니다"))
}

#[cfg(windows)]
fn run_host(home: &Path, mut s: Session, launch: Launch, mut queued: Option<(String, bool)>, no_process: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>, #[cfg(test)] ready_delay: Duration) -> io::Result<()> {
    use std::sync::{Arc, Mutex};
    let folder = dir(home, &s.id)?;
    let result = (|| -> io::Result<()> {
        let label = s.run_id.as_ref().zip(s.card_id.as_ref()).map(|(run, card)| format!("{run}-{card}"));
        let mut pty = match crate::sys::pty::Pty::spawn(&launch, s.cols, s.rows, label.as_deref()) {
            Ok(pty) => pty,
            Err(error) => { if let Some(flag) = no_process { flag.store(true, std::sync::atomic::Ordering::Release); } return Err(error); }
        };
        let mut input = pty.input.take().ok_or_else(|| io::Error::other("터미널 입력을 열지 못했습니다"))?;
        let mut output = pty.output.take().ok_or_else(|| io::Error::other("터미널 출력을 열지 못했습니다"))?;
        let (input_tx, input_rx) = std::sync::mpsc::sync_channel::<(Vec<u8>, bool)>(4);
        let (failure_tx, failure_rx) = std::sync::mpsc::channel();
        let paste_folder = folder.clone();
        std::thread::spawn(move || {
            for (bytes, paste) in input_rx {
                let result = input.write_all(&bytes).and_then(|()| {
                    if paste {
                        std::thread::sleep(Duration::from_millis(100));
                        input.write_all(b"\r")?;
                        OpenOptions::new().write(true).create_new(true).open(paste_folder.join("pasted"))?;
                    }
                    Ok(())
                });
                if let Err(error) = result { let _ = failure_tx.send(error); break; }
            }
        });
        let observed = Arc::new(Mutex::new(None::<Instant>));
        let seen = observed.clone();
        let out_dir = folder.clone();
        let reader = std::thread::spawn(move || -> io::Result<()> {
            let mut offset = 0u64;
            let mut buffer = [0u8; 8192];
            loop {
                let n = match output.read(&mut buffer) { Ok(0) => break, Ok(n) => n, Err(e) if e.kind() == io::ErrorKind::BrokenPipe => break, Err(e) => return Err(e) };
                let path = out_dir.join("output").join(format!("{offset:020}"));
                let tmp = path.with_extension("tmp");
                fs::write(&tmp, &buffer[..n])?;
                fs::rename(tmp, path)?;
                offset += n as u64;
                *seen.lock().map_err(|_| io::Error::other("터미널 잠금 실패"))? = Some(Instant::now());
                trim_output(&out_dir, offset)?;
            }
            Ok(())
        });
        let pid = std::process::id();
        let started = proc::started_at(pid).ok_or_else(|| io::Error::other("호스트 프로세스 식별 실패"))?;
        #[cfg(test)]
        std::thread::sleep(ready_delay);
        save_json(&folder.join("host.json"), &Host { pid, started, worker: Some(pty.pid()), job: pty.job_name().map(str::to_string) })?;
        let mut deadline = queued.as_ref().filter(|(_, confirm)| !confirm).map(|_| Instant::now() + Duration::from_secs(60));
        let code = loop {
            if let Some(code) = pty.try_wait()? { break code; }
            if let Ok(error) = failure_rx.try_recv() { return Err(error); }
            let queue = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(folder.join("input.queue"))?;
            let mut input_backlog = true;
            if queue.try_lock().is_ok() {
                input_backlog = false;
                let mut inputs: Vec<_> = fs::read_dir(folder.join("inputs"))?.flatten().collect();
                inputs.sort_by_key(|entry| entry.file_name());
                for entry in inputs {
                    if entry.file_name().to_string_lossy().starts_with('.') { fs::remove_file(entry.path())?; continue; }
                    let Some(bytes) = queued_bytes(&entry) else { continue };
                    match input_tx.try_send((bytes, false)) {
                        Ok(()) => fs::remove_file(entry.path())?,
                        Err(std::sync::mpsc::TrySendError::Full(_)) => { input_backlog = true; break; },
                        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => return Err(io::Error::other("터미널 입력이 종료됐습니다")),
                    }
                }
            }
            drop(queue);
            let mut requests: Vec<_> = fs::read_dir(folder.join("requests"))?.flatten().filter(|e| !e.file_name().to_string_lossy().starts_with('.')).collect();
            requests.sort_by_key(|e| e.file_name());
            let mut stopping = false;
            for entry in requests {
                let name = entry.file_name().to_string_lossy().into_owned();
                // Any process of this user can drop a file here: one that is no request must not end the host.
                let Some(bytes) = queued_bytes(&entry) else { continue };
                if name.ends_with("-input") {
                    match input_tx.try_send((bytes, false)) {
                        Ok(()) => {},
                        Err(std::sync::mpsc::TrySendError::Full(_)) => continue,
                        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => return Err(io::Error::other("터미널 입력이 종료됐습니다")),
                    }
                }
                else if name.ends_with("-resize") {
                    let size = serde_json::from_slice::<(u16, u16)>(&bytes).ok().filter(|(cols, rows)| dimensions(Some(*cols), Some(*rows)).is_ok());
                    if let Some((cols, rows)) = size.filter(|(cols, rows)| pty.resize(*cols, *rows).is_ok()) { s.cols = cols; s.rows = rows; save_json(&folder.join("session.json"), &s)?; }
                }
                else if name.ends_with("-paste") { if let Some((_, confirm)) = queued.as_mut() { *confirm = false; deadline = Some(Instant::now() + Duration::from_secs(60)); s.state = "running".into(); save_json(&folder.join("session.json"), &s)?; } }
                else if name.ends_with("-close") { pty.kill()?; stopping = true; }
                fs::remove_file(entry.path())?;
            }
            if stopping { continue; }
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) && !folder.join("pasted").is_file() { pty.kill()?; continue; }
            if let Some((line, false)) = queued.as_ref().filter(|_| !input_backlog) {
                let last = *observed.lock().map_err(|_| io::Error::other("터미널 잠금 실패"))?;
                if last.is_some_and(|at| at.elapsed() >= Duration::from_secs(1)) {
                    let bytes = format!("\x1b[200~{line}\x1b[201~").into_bytes();
                    match input_tx.try_send((bytes, true)) {
                        Ok(()) => queued = None,
                        Err(std::sync::mpsc::TrySendError::Full(_)) => {},
                        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => return Err(io::Error::other("터미널 입력이 종료됐습니다")),
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let confirmed = pty.stop_and_confirm();
        drop(input_tx);
        drop(pty);
        let output_result = reader.join().map_err(|_| io::Error::other("터미널 출력 스레드 실패"))?;
        output_result?;
        if confirmed { OpenOptions::new().write(true).create_new(true).open(folder.join("tree-exited"))?; }
        save_json(&folder.join("exit.json"), &Some(code))?;
        Ok(())
    })();
    if result.is_err() { let _ = save_json(&folder.join("exit.json"), &None::<i64>); }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_ids_never_escape() {
        assert!(is_session_id("t-20261111-093000-ab12"));
        for id in ["../x", "t-20261111-093000-a/12", "t-20261111-093000-ab12/..", "t-20261111-093000"] { assert!(!is_session_id(id)); }
    }

    #[test]
    fn reads_offsets_and_reports_trimmed_start() {
        let home = std::env::temp_dir().join(format!("project-o-term-read-{}", crate::run::store::new_run_id(Utc::now())));
        let s = create(&home, session("test".into(), "test".into(), "card", 80, 24)).unwrap();
        let output = dir(&home, &s.id).unwrap().join("output");
        fs::write(output.join(format!("{:020}", 3)), b"def").unwrap();
        fs::write(output.join(format!("{:020}", 6)), b"ghi").unwrap();
        let chunk = read(&home, &s.id, 1).unwrap();
        assert_eq!((chunk.start, chunk.offset), (3, 9));
        assert_eq!(decode_data(&chunk.data).unwrap(), b"defghi");
        let chunk = read(&home, &s.id, 5).unwrap();
        assert_eq!(decode_data(&chunk.data).unwrap(), b"fghi");
    }

    #[test]
    fn output_limit_removes_old_chunks() {
        let home = std::env::temp_dir().join(format!("project-o-term-trim-{}", crate::run::store::new_run_id(Utc::now())));
        let s = create(&home, session("test".into(), "test".into(), "card", 80, 24)).unwrap();
        let folder = dir(&home, &s.id).unwrap();
        let output = folder.join("output");
        fs::write(output.join(format!("{:020}", 0)), b"old").unwrap();
        let next = output.join(format!("{:020}", 3));
        fs::File::create(next).unwrap().set_len(MAX_OUTPUT).unwrap();
        trim_output(&folder, MAX_OUTPUT + 3).unwrap();
        assert_eq!(output_files(&folder).unwrap()[0].0, 3);
    }

    #[test]
    fn concurrent_writes_keep_each_writers_order_and_limit_size() {
        let home = std::env::temp_dir().join(format!("project-o-term-input-{}", crate::run::store::new_run_id(Utc::now())));
        let s = create(&home, session("test".into(), "test".into(), "card", 80, 24)).unwrap();
        assert!(write(&home, &s.id, &vec![0; 65537]).unwrap_err().to_string().contains("64 KiB"));
        std::thread::scope(|scope| for writer in 0..8 {
            let home = &home; let id = &s.id;
            scope.spawn(move || for sequence in 0..50 { write(home, id, format!("{writer}:{sequence}").as_bytes()).unwrap(); });
        });
        let mut inputs: Vec<_> = fs::read_dir(dir(&home, &s.id).unwrap().join("inputs")).unwrap().flatten().collect();
        inputs.sort_by_key(|entry| entry.file_name());
        let mut counts = [0; 8];
        for (sequence, entry) in inputs.iter().enumerate() {
            assert_eq!(entry.file_name().to_string_lossy().parse::<usize>().unwrap(), sequence);
            let text = fs::read_to_string(entry.path()).unwrap();
            let (writer, sequence) = text.split_once(':').unwrap();
            let writer: usize = writer.parse().unwrap();
            assert_eq!(sequence.parse::<usize>().unwrap(), counts[writer]);
            counts[writer] += 1;
        }
        assert_eq!(counts, [50; 8]);
    }

    #[test]
    fn unavailable_output_returns_empty_chunk() {
        let home = std::env::temp_dir().join(format!("project-o-term-no-output-{}", crate::run::store::new_run_id(Utc::now())));
        let s = create(&home, session("test".into(), "test".into(), "card", 80, 24)).unwrap();
        fs::remove_dir(dir(&home, &s.id).unwrap().join("output")).unwrap();
        let error = read(&home, &s.id, 42).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    }
}
