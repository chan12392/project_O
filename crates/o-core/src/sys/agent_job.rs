//! Whether this process runs inside an agent O started (ADR-0043).
//!
//! The runner marks a card and a lead turn with environment variables, which the agent itself
//! can clear. On Windows every agent is also started inside a named job object it cannot leave
//! (`win_launch::kill_on_close_job`: no breakaway), and so is every process it starts. The job
//! names are read from the system's object directory, not from a file an agent could remove.
//! Elsewhere only the variables tell.
//!
//! Limit: a process the agent has another service start for it (a scheduled task, WMI) is
//! outside the job. That needs an agent that already runs without a sandbox or approvals.

/// The job a card's CLI runs in: `Global\project_O-job-<run>-<card>-<random hex>`.
#[cfg_attr(not(windows), allow(dead_code))] // read only by the Windows lookup
pub(crate) const CARD_JOB: &str = "project_O-job-";
/// The job a lead's CLI runs in (a turn or a lead terminal): `Global\project_O-lead-<random hex>`.
#[cfg_attr(not(windows), allow(dead_code))] // read only by the Windows lookup
pub(crate) const LEAD_JOB: &str = "project_O-lead-";

/// The job this process runs in.
#[derive(Clone, Debug, PartialEq)]
pub enum AgentJob {
    /// A card's, with its label (`card_label`).
    Card(String),
    Lead,
}

/// How a run and a card are spelled in the card's job name.
pub fn card_label(run_id: &str, card_id: &str) -> String {
    label(&format!("{run_id}-{card_id}"))
}

pub(crate) fn label(text: &str) -> String {
    text.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(100).collect()
}

/// What a job name says, without `Global\`: the 32 random hex digits end it.
#[cfg_attr(not(windows), allow(dead_code))] // read only by the Windows lookup
fn read_name(name: &str) -> Option<AgentJob> {
    let hex = |text: &str| text.len() == 32 && text.bytes().all(|c| c.is_ascii_hexdigit());
    if let Some(rest) = name.strip_prefix(CARD_JOB) {
        let (label, random) = rest.rsplit_once('-')?;
        return hex(random).then(|| AgentJob::Card(label.to_string()));
    }
    name.strip_prefix(LEAD_JOB).filter(|random| hex(random)).map(|_| AgentJob::Lead)
}

/// The agent job this process is in, if any. A card's job wins over a lead's: a card a lead's
/// plan started is not in the lead's job, but should both ever hold, the card may do less.
pub fn current() -> Option<AgentJob> {
    static FOUND: std::sync::OnceLock<Option<AgentJob>> = std::sync::OnceLock::new();
    FOUND.get_or_init(find).clone()
}

#[cfg(not(windows))]
fn find() -> Option<AgentJob> {
    None
}

#[cfg(windows)]
fn find() -> Option<AgentJob> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::JobObjects::{IsProcessInJob, OpenJobObjectW};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    const JOB_OBJECT_QUERY: u32 = 0x0004;
    let me = unsafe { GetCurrentProcess() };
    let mut in_any = 0;
    if unsafe { IsProcessInJob(me, std::ptr::null_mut(), &mut in_any) } == 0 || in_any == 0 { return None; }
    let mut found = None;
    for name in windows::job_names() {
        let Some(job) = read_name(&name) else { continue };
        let wide: Vec<u16> = format!("Global\\{name}").encode_utf16().chain(Some(0)).collect();
        let raw = unsafe { OpenJobObjectW(JOB_OBJECT_QUERY, 0, wide.as_ptr()) };
        if raw.is_null() { continue; }
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut inside = 0;
        if unsafe { IsProcessInJob(me, handle.as_raw_handle(), &mut inside) } == 0 || inside == 0 { continue; }
        if matches!(job, AgentJob::Card(_)) { return Some(job); }
        found = Some(job);
    }
    found
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::os::windows::io::{FromRawHandle, OwnedHandle, AsRawHandle};
    use windows_sys::Win32::Foundation::HANDLE;

    #[repr(C)]
    struct UnicodeString {
        length: u16,
        maximum_length: u16,
        buffer: *const u16,
    }

    #[repr(C)]
    struct ObjectAttributes {
        length: u32,
        root_directory: HANDLE,
        object_name: *const UnicodeString,
        attributes: u32,
        security_descriptor: *const c_void,
        security_quality_of_service: *const c_void,
    }

    /// `OBJECT_DIRECTORY_INFORMATION`.
    #[repr(C)]
    struct Entry {
        name: UnicodeString,
        type_name: UnicodeString,
    }

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtOpenDirectoryObject(handle: *mut HANDLE, access: u32, attributes: *const ObjectAttributes) -> i32;
        fn NtQueryDirectoryObject(handle: HANDLE, buffer: *mut c_void, length: u32, single_entry: u8, restart: u8, context: *mut u32, returned: *mut u32) -> i32;
    }

    const DIRECTORY_QUERY: u32 = 0x0001;
    const STATUS_MORE_ENTRIES: i32 = 0x0000_0105;

    unsafe fn text(string: &UnicodeString) -> String {
        if string.buffer.is_null() { return String::new(); }
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(string.buffer, usize::from(string.length) / 2) })
    }

    /// The names of the job objects in `\BaseNamedObjects`, where `Global\` names live, that
    /// start as an agent job's does. Empty when the directory cannot be read.
    pub(super) fn job_names() -> Vec<String> {
        let path: Vec<u16> = "\\BaseNamedObjects".encode_utf16().collect();
        let name = UnicodeString { length: (path.len() * 2) as u16, maximum_length: (path.len() * 2) as u16, buffer: path.as_ptr() };
        let attributes = ObjectAttributes {
            length: std::mem::size_of::<ObjectAttributes>() as u32,
            root_directory: std::ptr::null_mut(),
            object_name: &name,
            attributes: 0,
            security_descriptor: std::ptr::null(),
            security_quality_of_service: std::ptr::null(),
        };
        let mut raw: HANDLE = std::ptr::null_mut();
        if unsafe { NtOpenDirectoryObject(&mut raw, DIRECTORY_QUERY, &attributes) } < 0 || raw.is_null() { return Vec::new(); }
        let directory = unsafe { OwnedHandle::from_raw_handle(raw) };
        // u64 storage keeps the entries, which hold pointers, aligned.
        let mut buffer = vec![0u64; 8 * 1024];
        let bytes = (buffer.len() * 8) as u32;
        let mut names = Vec::new();
        let (mut context, mut restart) = (0u32, 1u8);
        loop {
            let mut returned = 0u32;
            let status = unsafe { NtQueryDirectoryObject(directory.as_raw_handle(), buffer.as_mut_ptr().cast(), bytes, 0, restart, &mut context, &mut returned) };
            if status < 0 { break; }
            restart = 0;
            let mut entry = buffer.as_ptr().cast::<Entry>();
            let mut read = 0;
            // The entries end with one whose name is empty.
            while unsafe { !(*entry).name.buffer.is_null() } {
                let (name, kind) = unsafe { (text(&(*entry).name), text(&(*entry).type_name)) };
                if kind == "Job" && (name.starts_with(super::CARD_JOB) || name.starts_with(super::LEAD_JOB)) { names.push(name); }
                entry = unsafe { entry.add(1) };
                read += 1;
            }
            if status != STATUS_MORE_ENTRIES || read == 0 { break; }
        }
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_name_tells_a_card_from_a_lead() {
        let random = "0123456789abcdef0123456789ABCDEF";
        assert_eq!(read_name(&format!("{CARD_JOB}{}-{random}", card_label("20261111-run_1", "build"))), Some(AgentJob::Card("20261111-run_1-build".into())));
        assert_eq!(read_name(&format!("{LEAD_JOB}{random}")), Some(AgentJob::Lead));
        assert_eq!(read_name(&format!("{CARD_JOB}run-card-{}", &random[1..])), None);
        assert_eq!(read_name(&format!("{LEAD_JOB}x-{random}")), None);
        assert_eq!(read_name("project_O-job-"), None);
        assert_eq!(card_label("a\\b/c d", "e:f"), "abcd-ef");
    }

    /// The test runner is not an agent; the directory read itself must work and find a job made here.
    #[cfg(windows)]
    #[test]
    fn an_agent_job_made_here_is_listed_by_the_system() {
        let name = crate::sys::win_launch::lead_job_name().unwrap();
        let (_job, kept) = crate::sys::win_launch::kill_on_close_job(Some(name.clone())).unwrap();
        assert_eq!(kept.as_deref(), Some(name.as_str()));
        let listed = windows::job_names();
        assert!(listed.iter().any(|listed| format!("Global\\{listed}") == name), "{name} not in {listed:?}");
        assert!(listed.iter().all(|listed| listed.starts_with(CARD_JOB) || listed.starts_with(LEAD_JOB)));
    }
}
