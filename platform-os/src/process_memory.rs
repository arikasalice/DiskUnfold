//! Process-lifetime peak resident memory for opt-in measurements.

/// Returns the process peak resident memory in bytes when the operating system exposes it.
#[cfg(windows)]
pub fn peak_rss_bytes() -> Option<u64> {
    use windows::Win32::System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        Threading::GetCurrentProcess,
    };

    let mut counters = unsafe { std::mem::zeroed::<PROCESS_MEMORY_COUNTERS>() };
    counters.cb = u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS>()).ok()?;
    // SAFETY: the pseudo-handle is valid for this process and the writable counter struct has
    // the byte size passed to the API.
    unsafe {
        GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb).ok()?;
    }
    Some(counters.PeakWorkingSetSize as u64)
}

/// Returns the process high-water RSS from Linux procfs, in bytes.
#[cfg(target_os = "linux")]
pub fn peak_rss_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let value = status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))?;
    let kibibytes = value.split_whitespace().next()?.parse::<u64>().ok()?;
    kibibytes.checked_mul(1024)
}

/// Returns the process high-water RSS reported by `getrusage`, in bytes.
#[cfg(target_os = "macos")]
pub fn peak_rss_bytes() -> Option<u64> {
    let mut usage = unsafe { std::mem::zeroed::<libc::rusage>() };
    // SAFETY: `usage` is a valid writable output struct and RUSAGE_SELF selects this process.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } != 0 || usage.ru_maxrss < 0 {
        return None;
    }
    u64::try_from(usage.ru_maxrss).ok()
}

/// Other targets do not provide a supported high-water RSS implementation.
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub fn peak_rss_bytes() -> Option<u64> {
    None
}
