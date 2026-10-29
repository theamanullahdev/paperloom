//! The widgets process watches itself: if it ever uses far more processor or
//! memory than a few clocks and meters need (a bug, a broken look, a driver
//! fault), it quits rather than slow the computer down. Widgets are a nicety;
//! the user's machine comes first.

use std::time::{Duration, Instant};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

const CHECK_EVERY: Duration = Duration::from_secs(10);
/// Percent of one core, measured over `CHECK_EVERY`.
const MAX_CPU: f32 = 40.0;
/// How many checks in a row may be over `MAX_CPU` (half a minute).
const STRIKES: u32 = 3;
const MAX_MEMORY: u64 = 400 * 1024 * 1024;

pub struct Guard {
    system: System,
    pid: Option<Pid>,
    last: Instant,
    strikes: u32,
}

impl Guard {
    pub fn new() -> Self {
        Self { system: System::new(), pid: sysinfo::get_current_pid().ok(), last: Instant::now(), strikes: 0 }
    }

    /// Quits the process if it has been using too much for too long.
    pub fn check(&mut self) {
        if self.last.elapsed() < CHECK_EVERY {
            return;
        }
        self.last = Instant::now();
        let Some(pid) = self.pid else { return };
        self.system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, ProcessRefreshKind::nothing().with_cpu().with_memory());
        let Some(process) = self.system.process(pid) else { return };
        if process.memory() > MAX_MEMORY {
            quit(&format!("widgets used {} MB of memory; stopped to protect the computer", process.memory() / 1024 / 1024));
        }
        self.strikes = if process.cpu_usage() > MAX_CPU { self.strikes + 1 } else { 0 };
        if self.strikes >= STRIKES {
            quit(&format!("widgets used {:.0}% of a processor core for half a minute; stopped to protect the computer", process.cpu_usage()));
        }
    }
}

fn quit(reason: &str) -> ! {
    paperloom_common::log::error(reason);
    // Leaving closes every window and releases anything held (the pointer,
    // the lock file) with it.
    std::process::exit(3)
}
