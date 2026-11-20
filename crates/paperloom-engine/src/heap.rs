//! Keeps the engine's heap tight (Linux with glibc; elsewhere these do nothing).
//!
//! glibc gives every busy thread its own memory arena and keeps freed pages
//! in them. A player with a dozen threads ends up holding tens of megabytes
//! it is not using. Two arenas are plenty here, and handing freed pages back
//! once a minute costs a few microseconds. Playback is not touched.

use std::sync::Mutex;
use std::time::{Duration, Instant};

const EVERY: Duration = Duration::from_secs(60);
static LAST: Mutex<Option<Instant>> = Mutex::new(None);

/// At start-up, before the player makes its threads.
pub fn limit_arenas() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: mallopt only sets an allocator parameter.
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 2);
    }
}

/// On the engine tick: gives unused heap pages back, at most once a minute.
pub fn trim_now_and_then() {
    let mut last = LAST.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if last.is_some_and(|at| at.elapsed() < EVERY) {
        return;
    }
    *last = Some(Instant::now());
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: malloc_trim only returns free pages to the system.
    unsafe {
        libc::malloc_trim(0);
    }
}
