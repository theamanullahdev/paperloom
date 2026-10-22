//! Keeps the engine's own CPU and memory readings for the Usage page (see
//! `paperloom_common::usage_history`). One reading and one tiny file write
//! every 10 seconds, driven by the engine's existing 2-second tick.

use std::sync::Mutex;
use std::time::Instant;

use paperloom_common::usage_history::{self, Sample, EVERY, KEEP};

struct State {
    samples: Vec<Sample>,
    last: Instant,
    cpu_at_last: f64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

pub fn tick() {
    let mut slot = STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(cpu_now) = crate::perf_guard::cpu_seconds() else { return };
    let state = slot.get_or_insert_with(|| State { samples: Vec::new(), last: Instant::now(), cpu_at_last: cpu_now });
    let elapsed = state.last.elapsed();
    if elapsed < EVERY {
        return;
    }
    let cores = std::thread::available_parallelism().map_or(1.0, |n| n.get() as f64);
    let percent = ((cpu_now - state.cpu_at_last) / elapsed.as_secs_f64() / cores * 100.0) as f32;
    state.last = Instant::now();
    state.cpu_at_last = cpu_now;
    state.samples.push((percent.max(0.0), resident_bytes().unwrap_or(0)));
    if state.samples.len() > KEEP {
        state.samples.remove(0);
    }
    usage_history::save(&state.samples);
}

#[cfg(all(unix, not(target_os = "macos")))]
fn resident_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    Some(statm.split_whitespace().nth(1)?.parse::<u64>().ok()? * 4096)
}

#[cfg(windows)]
fn resident_bytes() -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: fills the struct we own for our own process.
    unsafe {
        let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        (GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) != 0).then_some(counters.WorkingSetSize as u64)
    }
}

#[cfg(target_os = "macos")]
fn resident_bytes() -> Option<u64> {
    None
}
