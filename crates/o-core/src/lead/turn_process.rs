use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::sys::proc::KillOnClose;

use super::STOP_POLL;

#[cfg(test)]
thread_local! { pub(super) static NO_JOB: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

struct State {
    child: crate::sys::worker::Child,
    tree: Option<Arc<Mutex<KillOnClose>>>,
    status: Option<ExitStatus>,
    unknown: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum End { Complete, Stop, Timeout, Kill }

pub(super) struct TurnProcess {
    state: Arc<Mutex<State>>,
    cancelled: Arc<AtomicBool>,
    signal: mpsc::Sender<End>,
    watcher: JoinHandle<End>,
    tree: Option<Arc<Mutex<KillOnClose>>>,
}

impl TurnProcess {
    pub(super) fn start(child: crate::sys::worker::Child, tree: Option<Arc<Mutex<KillOnClose>>>, stop: PathBuf, token: String, limit: Duration) -> Self {
        let state = Arc::new(Mutex::new(State { child, tree: tree.clone(), status: None, unknown: false }));
        let cancelled = Arc::new(AtomicBool::new(false));
        let (signal, events) = mpsc::channel();
        let (watched, interrupted) = (Arc::clone(&state), Arc::clone(&cancelled));
        let watched_tree = tree.clone();
        let watcher = thread::spawn(move || {
            let deadline = Instant::now() + limit;
            let reason = loop {
                if fs::read_to_string(&stop).is_ok_and(|t| t.is_empty() || t == token) { break End::Stop; }
                if Instant::now() >= deadline { break End::Timeout; }
                match events.recv_timeout(STOP_POLL.min(deadline.saturating_duration_since(Instant::now()))) {
                    Ok(reason) => break reason,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break End::Complete,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            };
            if reason != End::Complete {
                interrupted.store(true, Ordering::Release);
                if let Some(tree) = &watched_tree { if let Ok(mut tree) = tree.lock() { tree.stop_and_confirm(); } }
                // The same lock guards reap and termination; never signal a reaped PID.
                let mut state = watched.lock().unwrap();
                if state.status.is_none() && !state.unknown {
                    let tree = state.tree.clone();
                    match state.child.try_wait(&tree) {
                        Ok(Some(status)) => state.status = Some(status),
                        Ok(None) => { let _ = state.child.kill(); }
                        Err(_) => state.unknown = true,
                    }
                }
            }
            reason
        });
        Self { state, cancelled, signal, watcher, tree }
    }

    pub(super) fn reader(&self, mut pipe: impl Read + Send + 'static) -> (PipeReader, JoinHandle<()>) {
        let (send, receive) = mpsc::sync_channel(16);
        let worker = thread::spawn(move || {
            let mut buffer = [0; 8192];
            loop {
                match pipe.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => if send.send(Ok(buffer[..n].to_vec())).is_err() { break; },
                    Err(e) => { let _ = send.send(Err(e)); break; }
                }
            }
        });
        (PipeReader { receive, cancelled: Arc::clone(&self.cancelled), buffer: Vec::new(), offset: 0, remaining: None }, worker)
    }

    pub(super) fn writer(&self, mut pipe: impl Write + Send + 'static) -> (PipeWriter, JoinHandle<()>) {
        let (send, receive) = mpsc::channel::<(Vec<u8>, mpsc::Sender<io::Result<()>>)>();
        let worker = thread::spawn(move || {
            for (bytes, reply) in receive {
                let result = pipe.write_all(&bytes);
                let failed = result.is_err();
                let _ = reply.send(result);
                if failed { break; }
            }
        });
        (PipeWriter { send, cancelled: Arc::clone(&self.cancelled) }, worker)
    }

    pub(super) fn poll(&self) -> Option<ExitStatus> {
        let mut state = self.state.lock().unwrap();
        if state.status.is_none() && !state.unknown {
            let tree = state.tree.clone();
            match state.child.try_wait(&tree) {
                Ok(status) => state.status = status,
                Err(_) => state.unknown = true,
            }
        }
        state.status
    }

    pub(super) fn unknown(&self) -> bool { self.state.lock().unwrap().unknown }
    pub(super) fn cancelled(&self) -> bool { self.cancelled.load(Ordering::Acquire) }
    pub(super) fn kill(&self) { let _ = self.signal.send(End::Kill); }

    pub(super) fn finish(self) -> (End, Reap, bool) {
        let _ = self.signal.send(End::Complete);
        let reason = self.watcher.join().unwrap_or(End::Kill);
        let confirmed = self.tree.as_ref().is_some_and(|tree| tree.lock().is_ok_and(|mut tree| tree.stop_and_confirm()));
        (reason, Reap(self.state), confirmed)
    }
}

pub(super) struct PipeWriter {
    send: mpsc::Sender<(Vec<u8>, mpsc::Sender<io::Result<()>>)>,
    cancelled: Arc<AtomicBool>,
}

impl Write for PipeWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let (send, reply) = mpsc::channel();
        self.send.send((bytes.to_vec(), send)).map_err(|_| io::Error::other("총괄 입력이 닫혔습니다"))?;
        loop {
            match reply.recv_timeout(STOP_POLL) {
                Ok(result) => return result.map(|()| bytes.len()),
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err(io::Error::other("총괄 입력이 닫혔습니다")),
                Err(mpsc::RecvTimeoutError::Timeout) if self.cancelled.load(Ordering::Acquire) => return Err(io::Error::other("총괄 입력이 중단되었습니다")),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

pub(super) struct Reap(Arc<Mutex<State>>);

impl Reap {
    pub(super) fn wait(self) {
        loop {
            let mut state = self.0.lock().unwrap();
            let tree = state.tree.clone();
            if state.status.is_some() || state.child.try_wait(&tree).ok().flatten().is_some() { return; }
            drop(state);
            thread::sleep(Duration::from_secs(1));
        }
    }
}

pub(super) struct PipeReader {
    receive: mpsc::Receiver<io::Result<Vec<u8>>>,
    cancelled: Arc<AtomicBool>,
    buffer: Vec<u8>,
    offset: usize,
    remaining: Option<usize>,
}

impl Read for PipeReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() { return Ok(0); }
        while self.offset == self.buffer.len() {
            if self.cancelled.load(Ordering::Acquire) && self.remaining.is_none() { self.remaining = Some(16); }
            let received = if let Some(remaining) = &mut self.remaining {
                if *remaining == 0 { return Ok(0); }
                *remaining -= 1;
                self.receive.try_recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
            } else {
                self.receive.recv_timeout(STOP_POLL)
            };
            match received {
                Ok(Ok(buffer)) => { self.buffer = buffer; self.offset = 0; }
                Ok(Err(error)) => return Err(error),
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(0),
                Err(mpsc::RecvTimeoutError::Timeout) if self.cancelled.load(Ordering::Acquire) => return Ok(0),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
        let n = out.len().min(self.buffer.len() - self.offset);
        out[..n].copy_from_slice(&self.buffer[self.offset..self.offset + n]);
        self.offset += n;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Child, Command, Stdio};

    #[test]
    #[allow(clippy::zombie_processes)] // The fixture deliberately orphans its pipe-holding child.
    fn child_fixture() {
        let Ok(mode) = std::env::var("O_TEST_TURN_PROCESS") else { return };
        if mode == "exit" { return; }
        if mode == "mark" { fs::write(std::env::var_os("O_TEST_MARKER").unwrap(), "started").unwrap(); return; }
        if mode == "parent" {
            eprintln!("partial stderr");
            let mut command = Command::new(std::env::current_exe().unwrap());
            command.args(["--exact", "lead::turn_process::tests::child_fixture", "--nocapture"])
                .env("O_TEST_TURN_PROCESS", "descendant").stdin(Stdio::null()).stdout(Stdio::inherit());
            crate::sys::proc::hide_window(&mut command).spawn().unwrap();
        } else {
            thread::sleep(Duration::from_secs(3));
        }
    }

    fn child(mode: &str) -> Child {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "lead::turn_process::tests::child_fixture", "--nocapture"])
            .env("O_TEST_TURN_PROCESS", mode).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
        crate::sys::proc::hide_window(&mut command).spawn().unwrap()
    }

    #[test]
    fn no_job_pipe_is_interruptible_and_remains_unfinished_until_descendant_exits() {
        let mut child = child("parent");
        let stdout = child.stdout.take().unwrap();
        let process = TurnProcess::start(crate::sys::worker::Child::standard(child), None, PathBuf::new(), String::new(), Duration::from_millis(600));
        let (mut reader, worker) = process.reader(stdout);
        let mut output = Vec::new();
        reader.read_to_end(&mut output).unwrap();
        assert!(process.cancelled());
        assert!(!worker.is_finished(), "inherited pipe was incorrectly treated as closed");
        let (end, reap, _) = process.finish();
        assert!(end == End::Timeout);
        drop(reader);
        reap.wait();
        worker.join().unwrap();
    }

    #[test]
    fn unknown_process_is_not_killed_by_watcher() {
        let child = child("idle");
        let process = TurnProcess::start(crate::sys::worker::Child::standard(child), None, PathBuf::new(), String::new(), Duration::from_secs(5));
        process.state.lock().unwrap().unknown = true;
        process.kill();
        let (end, reap, _) = process.finish();
        assert!(end == End::Kill);
        let mut state = reap.0.lock().unwrap();
        assert!(state.child.try_wait(&None).unwrap().is_none());
        state.child.kill().unwrap();
        state.child.wait(&None).unwrap();
    }

    #[test]
    fn watcher_does_not_signal_after_child_is_reaped() {
        let exited = child("exit");
        let process = TurnProcess::start(crate::sys::worker::Child::standard(exited), None, PathBuf::new(), String::new(), Duration::from_secs(5));
        let deadline = Instant::now() + Duration::from_secs(3);
        while process.poll().is_none() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(20));
        }
        let mut bystander = child("idle");
        process.kill();
        let (end, _, _) = process.finish();
        assert!(end == End::Kill);
        assert!(bystander.try_wait().unwrap().is_none());
        bystander.kill().unwrap();
        bystander.wait().unwrap();
    }

    #[test]
    fn blocked_stdin_write_is_interrupted_by_deadline() {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "lead::turn_process::tests::child_fixture", "--nocapture"])
            .env("O_TEST_TURN_PROCESS", "idle").stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
        let mut child = crate::sys::proc::hide_window(&mut command).spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let process = TurnProcess::start(crate::sys::worker::Child::standard(child), None, PathBuf::new(), String::new(), Duration::from_millis(500));
        let (mut writer, worker) = process.writer(stdin);
        let started = Instant::now();
        assert!(writer.write_all(&vec![b'x'; 1024 * 1024]).is_err());
        assert!(started.elapsed() < Duration::from_secs(3));
        drop(writer);
        let (end, reap, _) = process.finish();
        assert!(end == End::Timeout);
        reap.wait();
        worker.join().unwrap();
    }

    #[test]
    fn no_job_unknown_survives_inherited_pipes_closing() {
        use crate::lead::{self, LeadEvent, SendRequest};
        use crate::run::plan::ProviderId;
        let root = std::env::temp_dir().join(format!("o-lead-no-job-{}", std::process::id()));
        let home = root.join("home");
        let project = root.join("project");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&project).unwrap();
        let project = project.to_string_lossy();
        let request = SendRequest { project: &project, provider: ProviderId::Claude, model: None, effort: None, text: "test", attach: &[], clean: None, fresh: false, yolo: false };
        let spawn = |_: &crate::providers::LaunchSpec, _: &str| {
            let mut command = crate::sys::launch::Launch::new(std::env::current_exe()?);
            command.args(["--exact", "lead::turn_process::tests::child_fixture", "--nocapture"]).env("O_TEST_TURN_PROCESS", "parent");
            Ok(command)
        };
        NO_JOB.set(true);
        let events = lead::send_within(&home, &request, &spawn, Duration::from_millis(600)).unwrap();
        NO_JOB.set(false);
        assert!(events.iter().any(|event| matches!(event, LeadEvent::Error { text, .. } if text.contains("불명"))), "{events:?}");
        assert!(events.iter().any(|event| matches!(event, LeadEvent::Error { text, .. } if text.contains("partial stderr"))), "{events:?}");
        let thread_id = lead::current_thread(&home, &project).unwrap();
        assert!(lead::running(&home, &thread_id).is_some());
        let deadline = Instant::now() + Duration::from_secs(5);
        while lead::running(&home, &thread_id).is_some() {
            assert!(Instant::now() < deadline, "turn did not recover after inherited pipes closed");
            thread::sleep(Duration::from_millis(25));
        }
        assert_eq!(lead::read_marker(&home, &thread_id).unwrap().unresolved, Some(true));
        assert!(lead::TurnSlot::take(&home, &thread_id, ProviderId::Claude).is_err());
        assert!(lead::lock_latest(&home, &format!("{thread_id}-p1")).is_err_and(|error| error.contains("불명")));
        assert_eq!(lead::clear_dead_turn(&home, &thread_id), Some(false));
        assert!(lead::running_path(&home, &thread_id).exists());
    }

    #[cfg(windows)]
    #[test]
    fn child_marker_failure_keeps_unknown_and_never_resumes() {
        use crate::lead::{self, SendRequest};
        use crate::run::plan::ProviderId;
        let root = std::env::temp_dir().join(format!("o-lead-marker-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap()));
        let home = root.join("home");
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();
        let marker = root.join("child-started");
        let helper = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("o-handle-owner.exe");
        assert!(helper.exists());
        let output = marker.clone();
        let spawn = move |_: &crate::providers::LaunchSpec, _: &str| {
            let mut launch = crate::sys::launch::Launch::new(std::env::current_exe()?);
            launch.handle_owner_executable = Some(helper.clone());
            launch.args(["--exact", "lead::turn_process::tests::child_fixture", "--nocapture"]).env("O_TEST_TURN_PROCESS", "mark").env("O_TEST_MARKER", &output);
            Ok(launch)
        };
        let project = project.to_string_lossy();
        let request = SendRequest { project: &project, provider: ProviderId::Claude, model: None, effort: None, text: "test", attach: &[], clean: None, fresh: false, yolo: false };
        super::super::FAIL_CHILD_MARKER.set(true);
        let result = lead::send(&home, &request, &spawn);
        super::super::FAIL_CHILD_MARKER.set(false);
        assert!(result.is_err());
        assert!(!marker.exists());
        let id = lead::current_thread(&home, &project).unwrap();
        assert_eq!(lead::read_marker(&home, &id).unwrap().unresolved, Some(true));
        assert_eq!(lead::show(&home, Some(&project))["thread"]["busy"], "unknown");
        assert!(lead::thread_for(&home, &project, true).is_err());
        assert_eq!(lead::clear_dead_turn(&home, &id), Some(false));
    }
}
