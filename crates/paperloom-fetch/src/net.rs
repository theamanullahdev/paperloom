//! Fast "is there internet at all?" check, so the UI can say "you're
//! offline" immediately instead of after a request times out.

use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_millis(1200);
// IP literals (Cloudflare, Quad9, Google) — no DNS lookup, which is the
// slow part of failing while offline.
const PROBES: [&str; 3] = ["1.1.1.1:443", "9.9.9.9:443", "8.8.8.8:443"];

pub fn is_online() -> bool {
    // No network interface up at all -> offline, instantly (no probe).
    if !any_interface_up() {
        return false;
    }

    let (sender, receiver) = mpsc::channel();
    for probe in PROBES {
        let sender = sender.clone();
        std::thread::spawn(move || {
            let reachable = probe.parse::<SocketAddr>().is_ok_and(|addr| TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).is_ok());
            let _ = sender.send(reachable);
        });
    }
    drop(sender);
    // First success wins; all-failed ends as soon as every probe reports.
    receiver.iter().any(|reachable| reachable)
}

/// Any non-loopback interface reporting `up`. If sysfs isn't readable we
/// can't tell, so assume yes and let the probe decide.
fn any_interface_up() -> bool {
    let Ok(entries) = std::fs::read_dir("/sys/class/net") else { return true };
    entries.flatten().any(|entry| {
        entry.file_name() != "lo"
            && std::fs::read_to_string(entry.path().join("operstate")).map(|state| state.trim() == "up").unwrap_or(false)
    })
}
