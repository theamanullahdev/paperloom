//! A process's own memory: what it holds that no other program shares.
//!
//! A process's "resident" size also counts the system libraries mapped into
//! it (graphics drivers, ffmpeg, …). Those pages exist once for the whole
//! computer, yet every program using them reports them in full. The private
//! figure is what closing the program would give back, which is what the
//! Usage page headlines. The shared part is shown next to it, never hidden.

/// Memory only this process holds, in bytes. `None` where the system can't say
/// (callers fall back to the resident size).
#[cfg(all(unix, not(target_os = "macos")))]
pub fn private_bytes(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let kilobytes = |name: &str| -> u64 {
        status.lines().find_map(|line| line.strip_prefix(name)).and_then(|rest| rest.trim().trim_end_matches("kB").trim().parse::<u64>().ok()).unwrap_or(0)
    };
    // Anonymous memory (heap, buffers) plus shared-memory segments it created.
    let total = kilobytes("RssAnon:") + kilobytes("RssShmem:");
    (total > 0).then_some(total * 1024)
}

#[cfg(windows)]
pub fn private_bytes(pid: u32) -> Option<u64> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: the handle is opened and closed here; the struct is ours and sized for the call.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut counters: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        let ok = GetProcessMemoryInfo(handle, std::ptr::from_mut(&mut counters).cast::<PROCESS_MEMORY_COUNTERS>(), counters.cb) != 0;
        CloseHandle(handle);
        ok.then_some(counters.PrivateUsage as u64)
    }
}

#[cfg(target_os = "macos")]
pub fn private_bytes(_pid: u32) -> Option<u64> {
    None
}

/// Private memory when the system reports it, else the resident size.
pub fn of(pid: u32, resident: u64) -> u64 {
    private_bytes(pid).unwrap_or(resident)
}

#[cfg(all(test, unix, not(target_os = "macos")))]
mod tests {
    #[test]
    fn own_private_memory_is_smaller_than_resident() {
        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        let resident: u64 = status.lines().find_map(|l| l.strip_prefix("VmRSS:")).unwrap().trim().trim_end_matches("kB").trim().parse().unwrap();
        let private = super::private_bytes(std::process::id()).unwrap();
        assert!(private > 0 && private <= resident * 1024);
    }
}
