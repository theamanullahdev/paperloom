//! Keeps the engine's own CPU use low without the user doing anything.
//!
//! Every ~2 s tick it adds up the process's CPU time; every 30 s it turns
//! that into "percent of one core". If a hardware-decoded wallpaper stays
//! above ~25% (or a software-decoded one above ~80%), decoding quality is
//! lowered one step — first cheaper filtering, then skipping non-reference
//! frames — and the new level is published for the Developers page. The
//! level resets when another wallpaper starts. Costs two reads of a cheap
//! counter per tick and a division every 30 s.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use libmpv2::Mpv;

const WINDOW: Duration = Duration::from_secs(30);
const MAX_LEVEL: u8 = 2;

struct Guard {
    level: u8,
    started: Instant,
    cpu_at_start: f64,
}

static GUARD: Mutex<Option<Guard>> = Mutex::new(None);

/// The level in force (0 = full quality).
pub fn level() -> u8 {
    GUARD.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_ref().map_or(0, |guard| guard.level)
}

/// A new wallpaper starts at full quality again.
pub fn reset() {
    *GUARD.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

/// Called on the engine tick. `paused`: nothing is decoding, so nothing to judge.
pub fn tick(mpv: &Mpv, enabled: bool, paused: bool, hardware: bool) {
    let mut slot = GUARD.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(now_cpu) = cpu_seconds() else { return };
    let guard = slot.get_or_insert_with(|| Guard { level: 0, started: Instant::now(), cpu_at_start: now_cpu });

    if !enabled || paused {
        guard.started = Instant::now();
        guard.cpu_at_start = now_cpu;
        return;
    }
    let elapsed = guard.started.elapsed();
    if elapsed < WINDOW {
        return;
    }
    let percent = (now_cpu - guard.cpu_at_start) / elapsed.as_secs_f64() * 100.0;
    guard.started = Instant::now();
    guard.cpu_at_start = now_cpu;

    let limit = if hardware { 25.0 } else { 80.0 };
    if percent > limit && guard.level < MAX_LEVEL {
        guard.level += 1;
        apply(mpv, guard.level);
        paperloom_common::log::warn(&format!("performance guard: {percent:.0}% of a core over 30 s; quality step {}", guard.level));
    } else {
        paperloom_common::log::debug(&format!("performance guard: {percent:.0}% of a core (limit {limit:.0}%), level {}", guard.level));
    }
}

fn apply(mpv: &Mpv, level: u8) {
    let set = |name: &str, value: &str| {
        let _ = mpv.set_property(name, value.to_string());
    };
    if level >= 1 {
        set("vd-lavc-fast", "yes");
        set("vd-lavc-skiploopfilter", "nonref");
    }
    if level >= 2 {
        set("vd-lavc-skipframe", "nonref");
    }
}

/// Total CPU time this process has used, in seconds.
#[cfg(all(unix, not(target_os = "macos")))]
fn cpu_seconds() -> Option<f64> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    // The command name may contain spaces; fields after the last ')' are fixed.
    let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    let ticks: f64 = fields.get(11)?.parse::<f64>().ok()? + fields.get(12)?.parse::<f64>().ok()?;
    Some(ticks / 100.0) // Linux's clock tick is 100 Hz.
}

#[cfg(windows)]
fn cpu_seconds() -> Option<f64> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: the pseudo-handle is always valid; the out-pointers are live locals.
    let ok = unsafe { GetProcessTimes(GetCurrentProcess(), &mut created, &mut exited, &mut kernel, &mut user) } != 0;
    let units = |time: FILETIME| ((u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)) as f64;
    ok.then(|| (units(kernel) + units(user)) / 10_000_000.0)
}

#[cfg(target_os = "macos")]
fn cpu_seconds() -> Option<f64> {
    None
}
