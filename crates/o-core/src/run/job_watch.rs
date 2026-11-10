//! A detached Windows watcher confirms a dead runner's job is empty before recording its exit.
use std::collections::HashMap;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::{ERROR_INVALID_PARAMETER, ERROR_MORE_DATA, FILETIME, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::JobObjects::{IsProcessInJob, JobObjectBasicAccountingInformation, JobObjectBasicProcessIdList, OpenJobObjectW, QueryInformationJobObject, TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_BASIC_PROCESS_ID_LIST};
use windows_sys::Win32::System::Threading::{GetProcessTimes, OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE};

use super::index::{self, sql};
use crate::sys::proc;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Lease {
    slot: i64,
    run: String,
    card: String,
    job: String,
    worker: u32,
    worker_started: i64,
    owner: u32,
    owner_started: i64,
}

pub(crate) fn spawn(home: &Path, slot: i64, executable: &Path) -> io::Result<()> {
    let lease = index::open(home)?.query_row(
        "SELECT slot_id, run_id, card_id, job_name, worker_pid, worker_started, owner_pid, owner_started FROM resource_leases WHERE slot_id = ?1 AND spawn_intent = 1 AND exit_confirmed IS NOT 1 AND job_name IS NOT NULL AND worker_started IS NOT NULL AND owner_started IS NOT NULL",
        [slot], |r| Ok(Lease { slot: r.get(0)?, run: r.get(1)?, card: r.get(2)?, job: r.get(3)?, worker: r.get(4)?, worker_started: r.get(5)?, owner: r.get(6)?, owner_started: r.get(7)? }),
    ).optional().map_err(sql)?;
    if let Some(lease) = lease {
        let payload = serde_json::to_string(&lease).map_err(io::Error::other)?;
        proc::spawn_detached(Command::new(executable).arg("_job-watch").arg(home).arg(payload))?;
    }
    Ok(())
}

/// Hidden command arguments contain only the state home and the exact lease/process identity.
pub fn run(args: &[String]) -> io::Result<()> {
    let [home, payload] = args else { return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid job-watch arguments")) };
    let home = Path::new(home);
    let lease: Lease = serde_json::from_str(payload).map_err(io::Error::other)?;
    if !home.is_absolute() || lease.slot <= 0 || !super::store::is_run_id(&lease.run) || !super::store::is_run_id(&lease.card)
        || lease.worker == 0 || lease.owner == 0 || lease.worker_started <= 0 || lease.owner_started <= 0
        || !lease.job.starts_with("Global\\project_O-job-") || lease.job.contains('\0') {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid job-watch identity"));
    }
    let (job, owner, worker) = open(home, &lease)?;
    watch(home, &lease, job, owner, worker, || {})
}

fn verified_process(pid: u32, expected: i64) -> io::Result<OwnedHandle> {
    let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, 0, pid) };
    if raw.is_null() { return Err(io::Error::last_os_error()); }
    let process = unsafe { OwnedHandle::from_raw_handle(raw) };
    let (mut created, mut exited, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    if unsafe { GetProcessTimes(raw, &mut created, &mut exited, &mut kernel, &mut user) } == 0 { return Err(io::Error::last_os_error()); }
    if Some((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime)) != u64::try_from(expected).ok() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "job-watch process identity changed"));
    }
    Ok(process)
}

fn open(home: &Path, lease: &Lease) -> io::Result<(OwnedHandle, OwnedHandle, Option<OwnedHandle>)> {
    // JOB_OBJECT_QUERY | JOB_OBJECT_TERMINATE (winnt.h); no permission to change the job's limits.
    let name: Vec<u16> = lease.job.encode_utf16().chain([0]).collect();
    let raw = unsafe { OpenJobObjectW(0x0004 | 0x0008, 0, name.as_ptr()) };
    if raw.is_null() { return Err(io::Error::last_os_error()); }
    let job = unsafe { OwnedHandle::from_raw_handle(raw) };
    let matches: bool = index::open(home)?.query_row(
        "SELECT EXISTS (SELECT 1 FROM resource_leases WHERE slot_id = ?1 AND run_id = ?2 AND card_id = ?3 AND job_name = ?4 AND worker_pid = ?5 AND worker_started = ?6 AND owner_pid = ?7 AND owner_started = ?8 AND spawn_intent = 1 AND exit_confirmed IS NOT 1)",
        params![lease.slot, lease.run, lease.card, lease.job, lease.worker, lease.worker_started, lease.owner, lease.owner_started], |r| r.get(0),
    ).map_err(sql)?;
    if !matches { return Err(io::Error::other("job-watch lease changed")); }
    let owner = verified_process(lease.owner, lease.owner_started)?;
    let worker = match verified_process(lease.worker, lease.worker_started) {
        Ok(worker) => {
            match unsafe { WaitForSingleObject(worker.as_raw_handle(), 0) } {
                WAIT_OBJECT_0 => {}, // The still-matching lease and the opened job identify an exited worker.
                WAIT_TIMEOUT => {
                    let mut in_job = 0;
                    if unsafe { IsProcessInJob(worker.as_raw_handle(), raw, &mut in_job) } == 0 { return Err(io::Error::last_os_error()); }
                    if in_job == 0 { return Err(io::Error::other("job-watch worker is not in the job")); }
                }
                _ => return Err(io::Error::last_os_error()),
            }
            Some(worker)
        }
        // Only a missing process is allowed; denied access or a reused pid is not evidence.
        Err(error) if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) => None,
        Err(error) => return Err(error),
    };
    Ok((job, owner, worker))
}

fn collect_processes(job: &OwnedHandle, handles: &mut HashMap<u32, OwnedHandle>) -> io::Result<()> {
    // usize storage gives the variable-length PID array its required alignment.
    let mut buffer = vec![0usize; std::mem::size_of::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() / std::mem::size_of::<usize>()];
    loop {
        let bytes = u32::try_from(std::mem::size_of_val(buffer.as_slice())).map_err(io::Error::other)?;
        let list = buffer.as_mut_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
        if unsafe { QueryInformationJobObject(job.as_raw_handle(), JobObjectBasicProcessIdList, list.cast(), bytes, std::ptr::null_mut()) } == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_MORE_DATA as i32) { return Err(error); }
            buffer.resize(buffer.len() * 2, 0);
            continue;
        }
        let pids = unsafe { std::slice::from_raw_parts(std::ptr::addr_of!((*list).ProcessIdList).cast::<usize>(), (*list).NumberOfProcessIdsInList as usize) };
        for &pid in pids {
            let pid = u32::try_from(pid).map_err(io::Error::other)?;
            if handles.contains_key(&pid) { continue; }
            let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, 0, pid) };
            if raw.is_null() {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) { continue; }
                return Err(error);
            }
            let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
            let mut in_job = 0;
            if unsafe { IsProcessInJob(raw, job.as_raw_handle(), &mut in_job) } == 0 { return Err(io::Error::last_os_error()); }
            if in_job == 0 { return Err(io::Error::other("job-watch listed process is not in the job")); }
            // Holding the handle also prevents this PID from being recycled between snapshots.
            handles.insert(pid, handle);
        }
        return Ok(());
    }
}

fn watch(home: &Path, lease: &Lease, job: OwnedHandle, owner: OwnedHandle, worker: Option<OwnedHandle>, before_confirm: impl FnOnce()) -> io::Result<()> {
    if unsafe { WaitForSingleObject(owner.as_raw_handle(), u32::MAX) } != WAIT_OBJECT_0 { return Err(io::Error::last_os_error()); }
    let mut processes = HashMap::new();
    if let Some(worker) = worker { processes.insert(lease.worker, worker); }
    collect_processes(&job, &mut processes)?;
    if unsafe { TerminateJobObject(job.as_raw_handle(), 1) } == 0 { return Err(io::Error::last_os_error()); }
    collect_processes(&job, &mut processes)?;
    loop {
        let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        if unsafe { QueryInformationJobObject(job.as_raw_handle(), JobObjectBasicAccountingInformation, (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(), std::mem::size_of_val(&info) as u32, std::ptr::null_mut()) } == 0 { return Err(io::Error::last_os_error()); }
        if info.ActiveProcesses == 0 { break; }
        // A stuck process must keep the lease unknown, however long its I/O takes to finish.
        std::thread::sleep(Duration::from_millis(50));
    }
    // ActiveProcesses can reach zero before process handles signal (observed on Windows).
    for process in processes.values() {
        if unsafe { WaitForSingleObject(process.as_raw_handle(), u32::MAX) } != WAIT_OBJECT_0 { return Err(io::Error::last_os_error()); }
    }
    before_confirm();
    index::open(home)?.execute(
        "UPDATE resource_leases SET exit_confirmed = 1 WHERE slot_id = ?1 AND run_id = ?2 AND card_id = ?3 AND job_name = ?4 AND worker_pid = ?5 AND worker_started = ?6 AND owner_pid = ?7 AND owner_started = ?8 AND spawn_intent = 1 AND exit_confirmed IS NOT 1",
        params![lease.slot, lease.run, lease.card, lease.job, lease.worker, lease.worker_started, lease.owner, lease.owner_started],
    ).map_err(sql)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::io::{BufRead, Write};
    use std::path::PathBuf;
    use std::process::Stdio;
    use crate::run::{plan::{Card, ProviderId}, store};
    use crate::sys::win_launch::{Child, Launch};

    struct Fixture {
        home: PathBuf,
        runner: std::process::Child,
        child: Child,
        grandchild: Option<OwnedHandle>,
        job: Option<proc::KillOnClose>,
        reservation: store::Reservation,
        lease: Lease,
    }

    impl Fixture {
        fn new(with_grandchild: bool) -> Self {
            let home = std::env::temp_dir().join(format!("o-job-watch-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap()));
            let project = home.join("project");
            std::fs::create_dir_all(&project).unwrap();
            let card = Card { id: "gui".into(), provider: ProviderId::Claude, role: None, task: "test".into(), model: None, effort: None, depends_on: vec![], resources: vec![] };
            let store::Reserved::Granted(reservation) = store::reserve_slot(&home, "run", &card, project.to_str().unwrap(), chrono::Utc::now()).unwrap() else { panic!("slot refused") };
            store::mark_spawning(&home, &reservation).unwrap();
            // Each process blocks on an open pipe; no sleep or elapsed time controls its lifetime.
            let root = std::env::var_os("SystemRoot").unwrap();
            let executable = PathBuf::from(&root).join("System32/findstr.exe");
            let runner = proc::hide_window(Command::new(&executable).arg("x")).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
            let mut launch = Launch { executable, args: vec![OsString::from("x")], cwd: project, environment: vec![(OsString::from("SystemRoot"), root)] };
            if with_grandchild {
                launch.executable = std::env::current_exe().unwrap();
                launch.args = ["--exact", "run::job_watch::tests::descendant_fixture", "--ignored", "--nocapture"].map(OsString::from).to_vec();
                launch.environment.push((OsString::from("O_JOB_WATCH_TEST_CHILD"), OsString::from("1")));
            }
            let (mut child, job) = launch.spawn_in_named_job(&runner, "run-gui").unwrap();
            let name = job.job_name().unwrap().to_string();
            store::link_slot(&home, &reservation, child.id(), Some(&name)).unwrap();
            let lease = Lease { slot: reservation.slot_row, run: "run".into(), card: "gui".into(), job: name, worker: child.id(), worker_started: proc::started_at(child.id()).unwrap().try_into().unwrap(), owner: runner.id(), owner_started: proc::started_at(runner.id()).unwrap().try_into().unwrap() };
            index::open(&home).unwrap().execute("UPDATE resource_leases SET owner_pid=?1, owner_started=?2 WHERE slot_id=?3", params![lease.owner, lease.owner_started, lease.slot]).unwrap();
            child.resume().unwrap();
            assert!(child.try_wait().unwrap().is_none());
            let grandchild = with_grandchild.then(|| {
                let reader = std::io::BufReader::new(child.stdout.take().unwrap());
                let pid: u32 = reader.lines().map(Result::unwrap).find_map(|line| line.split_once("watch-grandchild:").map(|(_, pid)| pid.trim().parse().unwrap())).expect("grandchild ready marker");
                let handle = verified_process(pid, proc::started_at(pid).unwrap().try_into().unwrap()).unwrap();
                let mut in_job = 0;
                assert_ne!(unsafe { IsProcessInJob(handle.as_raw_handle(), job.job, &mut in_job) }, 0);
                assert_eq!(in_job, 1, "the actual grandchild inherits the production job");
                assert_eq!(unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) }, WAIT_TIMEOUT);
                handle
            });
            Self { home, runner, child, grandchild, job: Some(job), reservation, lease }
        }

        fn start_watch(&self) -> std::thread::JoinHandle<io::Result<()>> {
            // Opening synchronously proves the watcher holds the job before the fake runner dies.
            let (job, owner, worker) = open(&self.home, &self.lease).unwrap();
            let home = self.home.clone();
            let lease = self.lease.clone();
            let mut children = vec![self.child.watch_handle().unwrap()];
            if let Some(grandchild) = &self.grandchild { children.push(grandchild.try_clone().unwrap()); }
            std::thread::spawn(move || watch(&home, &lease, job, owner, worker, || {
                for child in children {
                    assert_eq!(unsafe { WaitForSingleObject(child.as_raw_handle(), 0) }, WAIT_OBJECT_0, "EVERY process handle must be signaled BEFORE exit_confirmed is written");
                }
                assert_ne!(store::resource_leases(&home).unwrap().first().and_then(|l| l.exit_confirmed), Some(true));
            }))
        }

        fn kill_runner(&mut self) {
            self.runner.kill().unwrap();
            self.runner.wait().unwrap();
        }

        fn child_ended(&mut self) {
            let handle = self.child.watch_handle().unwrap();
            assert_eq!(unsafe { WaitForSingleObject(handle.as_raw_handle(), 60_000) }, WAIT_OBJECT_0, "process did not end");
            if let Some(grandchild) = &self.grandchild {
                assert_eq!(unsafe { WaitForSingleObject(grandchild.as_raw_handle(), 60_000) }, WAIT_OBJECT_0, "grandchild did not end");
            }
            self.child.wait().unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = self.runner.kill();
            let _ = self.runner.wait();
            drop(self.job.take());
            let _ = std::fs::remove_dir_all(&self.home);
        }
    }

    #[test]
    fn killed_runner_is_recoverable_only_after_the_job_ends() {
        let mut f = Fixture::new(true);
        let watcher = f.start_watch();
        assert!(f.child.try_wait().unwrap().is_none());
        let before = store::resource_leases(&f.home).unwrap();
        assert_ne!(before[0].exit_confirmed, Some(true));
        assert_eq!(before[0].state, "unknown");
        f.kill_runner();
        // Keep the runner's job handle open: the watcher itself must terminate the child.
        watcher.join().unwrap().unwrap();
        f.child_ended();
        let after = store::resource_leases(&f.home).unwrap();
        assert_eq!(after[0].exit_confirmed, Some(true));
        assert_eq!(after[0].state, "recoverable");
        // Recovery re-reads evidence in its transaction, rather than trusting the previous list.
        index::open(&f.home).unwrap().execute("UPDATE resource_leases SET exit_confirmed=NULL", []).unwrap();
        assert!(store::recover_resource(&f.home, f.lease.slot).is_err());
        index::open(&f.home).unwrap().execute("UPDATE resource_leases SET exit_confirmed=1", []).unwrap();
        store::recover_resource(&f.home, f.lease.slot).unwrap();
        assert!(!store::resource_held(&f.home, "run", "gui").unwrap());
        assert_eq!(store::started_on(&f.home, chrono::Utc::now()).unwrap(), 1);
    }

    #[test]
    fn changed_worker_or_job_is_not_confirmed() {
        for update in ["UPDATE resource_leases SET worker_pid=worker_pid+1", "UPDATE resource_leases SET job_name=job_name || '-changed'"] {
            let mut f = Fixture::new(false);
            let watcher = f.start_watch();
            index::open(&f.home).unwrap().execute(update, []).unwrap();
            f.kill_runner();
            watcher.join().unwrap().unwrap();
            let rows = store::resource_leases(&f.home).unwrap();
            assert_ne!(rows[0].exit_confirmed, Some(true));
            assert_eq!(rows[0].state, "unknown");
            assert!(store::recover_resource(&f.home, f.lease.slot).is_err());
        }
    }

    #[test]
    fn unrelated_worker_or_reused_runner_identity_is_not_trusted() {
        let f = Fixture::new(false);
        let mut wrong = f.lease.clone();
        wrong.owner_started += 1;
        assert!(open(&f.home, &wrong).is_err());
        index::open(&f.home).unwrap().execute("UPDATE resource_leases SET owner_started=owner_started+1", []).unwrap();
        assert!(open(&f.home, &wrong).is_err(), "the opened runner must match its creation time too");
        wrong = f.lease.clone();
        wrong.worker = f.lease.owner;
        wrong.worker_started = f.lease.owner_started;
        index::open(&f.home).unwrap().execute("UPDATE resource_leases SET owner_started=?1, worker_pid=?2, worker_started=?1", params![wrong.owner_started, wrong.worker]).unwrap();
        assert!(open(&f.home, &wrong).is_err(), "a live worker outside the named job is not trusted");
        assert!(f.child.try_wait().unwrap().is_none());
        assert_ne!(store::resource_leases(&f.home).unwrap()[0].exit_confirmed, Some(true));
    }

    #[test]
    fn missing_job_without_confirmation_stays_unknown() {
        let mut f = Fixture::new(false);
        drop(f.job.take());
        f.child_ended();
        f.kill_runner();
        assert_eq!(open(&f.home, &f.lease).unwrap_err().raw_os_error(), Some(windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND as i32));
        assert_eq!(store::resource_leases(&f.home).unwrap()[0].state, "unknown");
        assert!(store::recover_resource(&f.home, f.lease.slot).is_err());
    }

    #[test]
    fn normal_drop_ends_the_card_while_a_watcher_holds_the_job() {
        let mut f = Fixture::new(true);
        let watcher = f.start_watch();
        drop(f.job.take());
        f.child_ended();
        assert!(f.runner.try_wait().unwrap().is_none(), "the watcher is still waiting for the runner");
        assert_ne!(store::resource_leases(&f.home).unwrap()[0].exit_confirmed, Some(true));
        store::release_slot(&f.home, &f.reservation, true).unwrap();
        f.kill_runner();
        watcher.join().unwrap().unwrap();
        assert!(store::resource_leases(&f.home).unwrap().is_empty(), "normal release is never recreated");
    }

    #[test]
    fn an_exited_worker_is_matched_by_lease_and_open_job() {
        let mut f = Fixture::new(false);
        drop(f.child.stdin.take());
        f.child_ended();
        let watcher = f.start_watch();
        f.kill_runner();
        watcher.join().unwrap().unwrap();
        assert_eq!(store::resource_leases(&f.home).unwrap()[0].state, "recoverable");
    }

    #[test]
    #[ignore = "subprocess fixture: run only from the watcher tests"]
    fn descendant_fixture() {
        if std::env::var("O_JOB_WATCH_TEST_CHILD").as_deref() != Ok("1") { return; }
        let executable = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/findstr.exe");
        let mut child = proc::hide_window(Command::new(executable).arg("x")).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        println!("watch-grandchild:{}", child.id());
        std::io::stdout().flush().unwrap();
        child.wait().unwrap();
    }
}
