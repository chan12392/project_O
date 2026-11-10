//! How much of the machine's physical memory is in use, for the RAM cap on card starts.

/// Used physical memory in percent (0–100), or `None` where O cannot read it (macOS for now).
pub fn used_percent() -> Option<u8> {
    let (total, available) = totals()?;
    (total > 0).then(|| (100 - available.min(total) * 100 / total) as u8)
}

#[cfg(windows)]
fn totals() -> Option<(u64, u64)> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: `status` is a zeroed MEMORYSTATUSEX with its length set, as the call requires.
    (unsafe { GlobalMemoryStatusEx(&mut status) } != 0).then_some((status.ullTotalPhys, status.ullAvailPhys))
}

#[cfg(target_os = "linux")]
fn totals() -> Option<(u64, u64)> {
    parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn totals() -> Option<(u64, u64)> {
    None
}

/// `MemTotal` and `MemAvailable` from `/proc/meminfo`, in kB.
#[cfg(any(target_os = "linux", test))]
fn parse_meminfo(text: &str) -> Option<(u64, u64)> {
    let field = |name: &str| {
        text.lines().find_map(|line| line.strip_prefix(name)?.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
    };
    Some((field("MemTotal:")?, field("MemAvailable:")?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meminfo_gives_total_and_available() {
        let text = "MemTotal:       32000000 kB\nMemFree:         1000000 kB\nMemAvailable:    9600000 kB\n";
        assert_eq!(parse_meminfo(text), Some((32_000_000, 9_600_000)));
        assert_eq!(parse_meminfo("MemTotal: 1 kB\n"), None);
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn this_machine_reports_a_percent() {
        assert!(used_percent().is_some_and(|p| p <= 100));
    }
}
