//! Process-wide count of bytes received over HTTP, for the Downloads page's
//! speed meter and graph. One relaxed atomic — free to update per chunk.

use std::sync::atomic::{AtomicU64, Ordering};

static RECEIVED: AtomicU64 = AtomicU64::new(0);

pub fn add(bytes: u64) {
    RECEIVED.fetch_add(bytes, Ordering::Relaxed);
}

/// Total bytes received since the app started.
pub fn received() -> u64 {
    RECEIVED.load(Ordering::Relaxed)
}
