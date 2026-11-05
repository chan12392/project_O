//! Kernel accounting, not a sampled process tree, proves descendant exit.
use std::io;

unsafe extern "C" {
    fn proc_pidinfo(pid: i32, flavor: i32, arg: u64, buffer: *mut libc::c_void, size: i32) -> i32;
    fn proc_listallpids(buffer: *mut libc::c_void, size: i32) -> i32;
}

type Usage = unsafe extern "C" fn(u64, *mut libc::c_void, usize) -> i32;
type Signal = unsafe extern "C" fn(*const [u32; 8], i32) -> i32;

pub struct Coalition { pub id: u64, owner: u32, usage: Usage, signal: Signal }

pub fn id(pid: u32) -> io::Result<u64> {
    let mut ids = [0u64; 5];
    if unsafe { proc_pidinfo(pid as i32, 20, 0, ids.as_mut_ptr().cast(), 40) } != 40 || ids[0] == 0 {
        return Err(io::Error::other("macOS 작업 묶음을 확인하지 못했습니다"));
    }
    Ok(ids[0])
}

impl Coalition {
    pub fn isolated(owner: u32, outside: u32) -> io::Result<Self> {
        let usage = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"coalition_info_resource_usage".as_ptr()) };
        let signal = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"proc_signal_with_audittoken".as_ptr()) };
        if usage.is_null() || signal.is_null() { return Err(io::Error::other("이 macOS에서는 안전한 하위 작업 추적을 지원하지 않습니다")); }
        let result = Self { id: id(owner)?, owner, usage: unsafe { std::mem::transmute::<*mut libc::c_void, Usage>(usage) }, signal: unsafe { std::mem::transmute::<*mut libc::c_void, Signal>(signal) } };
        if result.id == id(outside)? || result.active()? != 1 { return Err(io::Error::other("독립된 macOS 작업 묶음이 아닙니다")); }
        Ok(result)
    }

    pub fn active(&self) -> io::Result<u64> {
        // The stable prefix contains tasks_started and tasks_exited. A generous
        // zeroed buffer supports the kernel's size-negotiated accounting ABI.
        let mut usage = [0u64; 64];
        if unsafe { (self.usage)(self.id, usage.as_mut_ptr().cast(), std::mem::size_of_val(&usage)) } != 0 { return Err(io::Error::last_os_error()); }
        usage[0].checked_sub(usage[1]).ok_or_else(|| io::Error::other("잘못된 macOS 작업 수"))
    }

    pub fn stop_members(&self) -> io::Result<()> {
        let size = unsafe { proc_listallpids(std::ptr::null_mut(), 0) };
        if size <= 0 { return Err(io::Error::last_os_error()); }
        let mut pids = vec![0i32; size as usize + 128];
        let count = unsafe { proc_listallpids(pids.as_mut_ptr().cast(), (pids.len() * 4) as i32) };
        if count < 0 { return Err(io::Error::last_os_error()); }
        for pid in pids.into_iter().take(count as usize).filter(|p| *p > 0 && *p as u32 != self.owner) {
            let mut identity = [0u64; 7];
            if unsafe { proc_pidinfo(pid, 17, 0, identity.as_mut_ptr().cast(), 56) } != 56 { continue; }
            // Capture pidversion BEFORE checking membership. A reused PID cannot
            // satisfy both checks and the final kernel audit-token lookup.
            let version = identity[4] as u32;
            if id(pid as u32).ok() != Some(self.id) { continue; }
            let mut token = [0u32; 8]; token[5] = pid as u32; token[7] = version;
            let result = unsafe { (self.signal)(&token, libc::SIGKILL) };
            if result != 0 && result != libc::ESRCH { return Err(io::Error::from_raw_os_error(result)); }
        }
        Ok(())
    }
}

/// Cleanup also runs on IPC/write/reap errors. Failure never becomes exit proof.
pub struct Cleanup<'a>(pub &'a Coalition);
impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while self.0.active().ok() != Some(1) && std::time::Instant::now() < deadline {
            let _ = self.0.stop_members();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audit_token_rejects_same_pid_with_wrong_generation() {
        let mut child = std::process::Command::new("/bin/sleep").arg("20").spawn().unwrap();
        let mut identity = [0u64; 7];
        assert_eq!(unsafe { proc_pidinfo(child.id() as i32, 17, 0, identity.as_mut_ptr().cast(), 56) }, 56);
        let address = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"proc_signal_with_audittoken".as_ptr()) };
        assert!(!address.is_null());
        let signal: Signal = unsafe { std::mem::transmute(address) };
        let mut token = [0; 8]; token[5] = child.id(); token[7] = (identity[4] as u32).wrapping_add(1);
        assert_eq!(unsafe { signal(&token, libc::SIGCONT) }, libc::ESRCH);
        token[7] = identity[4] as u32;
        assert_eq!(unsafe { signal(&token, libc::SIGCONT) }, 0);
        assert!(child.try_wait().unwrap().is_none()); child.kill().unwrap(); child.wait().unwrap();
    }
    #[test]
    fn shared_coalition_is_rejected() {
        assert!(Coalition::isolated(std::process::id(), std::process::id()).is_err());
    }
}
