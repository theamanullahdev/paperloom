//! Fast "is there internet at all?" check, so the UI can say "you're
//! offline" immediately instead of after a request times out.

use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_millis(1200);
const PATIENT_PROBE_TIMEOUT: Duration = Duration::from_millis(3500);
// IP literals (Cloudflare, Quad9, Google) — no DNS lookup, which is the
// slow part of failing while offline.
const PROBES: [&str; 3] = ["1.1.1.1:443", "9.9.9.9:443", "8.8.8.8:443"];

pub fn is_online() -> bool {
    // No network interface up at all -> offline, instantly (no probe).
    if !any_interface_up() {
        return false;
    }
    // A flaky link can drop one round of probes; only call it offline after
    // a second, more patient round also gets no answer.
    probe_round(PROBE_TIMEOUT) || probe_round(PATIENT_PROBE_TIMEOUT)
}

fn probe_round(timeout: Duration) -> bool {
    let (sender, receiver) = mpsc::channel();
    for probe in PROBES {
        let sender = sender.clone();
        std::thread::spawn(move || {
            let reachable = probe.parse::<SocketAddr>().is_ok_and(|addr| TcpStream::connect_timeout(&addr, timeout).is_ok());
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

/// How one site looks from here: name lookup time, then TCP connect time.
/// `None` = failed or timed out.
#[derive(Debug, Clone, PartialEq)]
pub struct HostCheck {
    pub host: String,
    pub dns_ms: Option<u32>,
    pub connect_ms: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Good,
    Slow,
    /// The internet answers, but turning names into addresses fails or
    /// times out — sites then look "broken" even though the link is fine.
    DnsProblem,
    /// A network interface is up but nothing on the internet answers.
    Offline,
    /// No network interface is up (Wi-Fi off, cable out).
    NoInterface,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnosis {
    pub verdict: Verdict,
    /// Fastest connect to a well-known public address (no DNS involved).
    pub internet_ms: Option<u32>,
    pub hosts: Vec<HostCheck>,
}

const DNS_TIMEOUT: Duration = Duration::from_secs(6);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);

/// Looks at the connection the way a person would debug it: is there a
/// link, does the internet answer, do names resolve, can each site be
/// reached. Takes up to ~6s when things are broken, so run it off the UI
/// thread.
pub fn diagnose(hosts: &[String]) -> Diagnosis {
    use std::net::ToSocketAddrs;
    use std::time::Instant;

    if !any_interface_up() {
        return Diagnosis { verdict: Verdict::NoInterface, internet_ms: None, hosts: Vec::new() };
    }

    // Internet reachability by IP literal, in parallel.
    let (sender, receiver) = mpsc::channel();
    for probe in PROBES {
        let sender = sender.clone();
        std::thread::spawn(move || {
            let started = Instant::now();
            let ok = probe.parse::<SocketAddr>().is_ok_and(|addr| TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).is_ok());
            let _ = sender.send(ok.then(|| started.elapsed().as_millis() as u32));
        });
    }
    drop(sender);

    // Each site: DNS, then connect to what it resolved to.
    let mut unique: Vec<String> = Vec::new();
    for host in hosts {
        if !host.is_empty() && !unique.contains(host) {
            unique.push(host.clone());
        }
    }
    let checks: Vec<_> = unique
        .into_iter()
        .map(|host| {
            let (sender, receiver) = mpsc::channel();
            let name = host.clone();
            std::thread::spawn(move || {
                let started = Instant::now();
                let addr = (name.as_str(), 443).to_socket_addrs().ok().and_then(|mut addrs| addrs.next());
                let dns_ms = addr.map(|_| started.elapsed().as_millis() as u32);
                let connect_ms = addr.and_then(|addr| {
                    let started = Instant::now();
                    TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).ok().map(|_| started.elapsed().as_millis() as u32)
                });
                let _ = sender.send((dns_ms, connect_ms));
            });
            (host, receiver)
        })
        .collect();

    let internet_ms = receiver.iter().flatten().min();

    let deadline = Instant::now() + DNS_TIMEOUT + CONNECT_TIMEOUT;
    let hosts: Vec<HostCheck> = checks
        .into_iter()
        .map(|(host, receiver)| {
            let wait = deadline.saturating_duration_since(Instant::now());
            let (dns_ms, connect_ms) = receiver.recv_timeout(wait).unwrap_or((None, None));
            HostCheck { host, dns_ms, connect_ms }
        })
        .collect();

    let verdict = if internet_ms.is_none() {
        Verdict::Offline
    } else if hosts.iter().any(|check| check.dns_ms.is_none()) {
        Verdict::DnsProblem
    } else if internet_ms.is_some_and(|ms| ms > 400)
        || hosts.iter().any(|check| check.dns_ms.is_some_and(|ms| ms > 1500) || check.connect_ms.is_none_or(|ms| ms > 2000))
    {
        Verdict::Slow
    } else {
        Verdict::Good
    };

    Diagnosis { verdict, internet_ms, hosts }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unresolvable_host_is_a_dns_problem_not_offline() {
        // `.invalid` never resolves (RFC 6761). Only meaningful with a
        // working link, so skip quietly on an offline machine.
        let diagnosis = diagnose(&["definitely-not-a-real-host.invalid".to_string()]);
        if diagnosis.internet_ms.is_some() {
            assert_eq!(diagnosis.verdict, Verdict::DnsProblem);
            assert_eq!(diagnosis.hosts[0].dns_ms, None);
        }
    }
}
