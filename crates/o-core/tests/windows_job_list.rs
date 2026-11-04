//! Windows-only feasibility probe; this does not replace the production spawner.
//! Run: cargo test -p o-core --test windows_job_list -- --test-threads=1
//! The owner is killed after CreateProcessW returns, before the child is resumed;
//! owner death inside CreateProcessW and production reservation recovery are not tested.
#![cfg(windows)]

use std::io::{BufRead, BufReader, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::JobObjects::{CreateJobObjectW, SetInformationJobObject, JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE};
use windows_sys::Win32::System::Threading::{CreateProcessW, DeleteProcThreadAttributeList, InitializeProcThreadAttributeList, OpenProcess, UpdateProcThreadAttribute, WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED, EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROC_THREAD_ATTRIBUTE_JOB_LIST, STARTUPINFOEXW};

struct Owner(Child);
impl Drop for Owner {
    fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
}

// Called in the suspended child only; it must never access the user's profiles.
#[test]
fn job_list_child() {
    if std::env::var_os("O_JOB_LIST_PROBE").is_some() {
        std::thread::sleep(Duration::from_secs(30));
    }
}

#[test]
fn owner_death_kills_suspended_job_member() {
    if std::env::var_os("O_JOB_LIST_PROBE").is_some() {
        unsafe { launch_suspended_member() };
        return;
    }

    check_owner_death(false);
}

#[test]
fn owner_death_kills_member_inside_nested_job() {
    check_owner_death(true);
}

fn check_owner_death(nested: bool) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", "owner_death_kills_suspended_job_member", "--nocapture"])
        .env_clear().env("O_JOB_LIST_PROBE", "1")
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW | if nested { CREATE_SUSPENDED } else { 0 });
    if let Some(root) = std::env::var_os("SystemRoot") { command.env("SystemRoot", root); }
    let mut owner = Owner(command.spawn().unwrap());
    // Keep the outer job open while the inner job's owner is killed.
    let _outer = if nested {
        let job = o_core::sys::proc::contain(&owner.0).expect("outer job assignment failed");
        o_core::sys::proc::resume_worker(owner.0.id()).expect("outer job member could not resume");
        Some(job)
    } else { None };
    let output = owner.0.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(output).lines().map_while(Result::ok) {
            if let Some((_, pid)) = line.split_once("O_JOB_PID=") {
                let _ = tx.send(pid.trim().parse::<u32>());
                break;
            }
        }
    });
    let pid = rx.recv_timeout(Duration::from_secs(15)).expect("owner did not report child").unwrap();
    // Hold the process object across owner death; this avoids PID-reuse guesses.
    let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    assert!(!raw.is_null(), "OpenProcess: {}", std::io::Error::last_os_error());
    let child = unsafe { OwnedHandle::from_raw_handle(raw) };
    assert_eq!(unsafe { WaitForSingleObject(child.as_raw_handle(), 0) }, WAIT_TIMEOUT);
    owner.0.kill().unwrap();
    owner.0.wait().unwrap();
    assert_eq!(unsafe { WaitForSingleObject(child.as_raw_handle(), 10_000) }, WAIT_OBJECT_0, "suspended child outlived job owner");
}

unsafe fn launch_suspended_member() {
    let raw_job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    assert!(!raw_job.is_null(), "CreateJobObjectW: {}", std::io::Error::last_os_error());
    let job = unsafe { OwnedHandle::from_raw_handle(raw_job) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    assert_ne!(unsafe { SetInformationJobObject(raw_job, JobObjectExtendedLimitInformation, (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(), std::mem::size_of_val(&limits) as u32) }, 0);
    let mut bytes = 0usize;
    unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut bytes) };
    assert!(bytes > 0);
    let mut storage = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
    let attrs = storage.as_mut_ptr().cast();
    assert_ne!(unsafe { InitializeProcThreadAttributeList(attrs, 1, 0, &mut bytes) }, 0);
    assert_ne!(unsafe { UpdateProcThreadAttribute(attrs, 0, PROC_THREAD_ATTRIBUTE_JOB_LIST as usize, (&raw_job as *const HANDLE).cast(), std::mem::size_of_val(&raw_job), std::ptr::null_mut(), std::ptr::null()) }, 0);
    let exe = std::env::current_exe().unwrap();
    let app: Vec<u16> = exe.as_os_str().encode_wide().chain([0]).collect();
    let mut cmd: Vec<u16> = format!("\"{}\" --exact job_list_child --nocapture", exe.display()).encode_utf16().chain([0]).collect();
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    startup.lpAttributeList = attrs;
    let mut info = PROCESS_INFORMATION::default();
    let ok = unsafe { CreateProcessW(app.as_ptr(), cmd.as_mut_ptr(), std::ptr::null(), std::ptr::null(), 0, CREATE_NO_WINDOW | CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT, std::ptr::null(), std::ptr::null(), &startup.StartupInfo, &mut info) };
    let error = std::io::Error::last_os_error();
    unsafe { DeleteProcThreadAttributeList(attrs) };
    assert_ne!(ok, 0, "CreateProcessW: {error}");
    println!("O_JOB_PID={}", info.dwProcessId);
    std::io::stdout().flush().unwrap();
    unsafe { CloseHandle(info.hThread); CloseHandle(info.hProcess); }
    // Keep the only job handle open until the parent forcibly kills this owner.
    std::thread::sleep(Duration::from_secs(30));
    drop(job);
}
