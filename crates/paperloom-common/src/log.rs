//! A tiny file logger and crash trail, shared by the app and the engine.
//!
//! One rotating text file per process (`gui.log`, `engine.log` lives apart
//! as the startup-failure note), 512 KB then one rotated copy, so logging
//! can never grow without bound. Panics are written to `crash.log` with a
//! backtrace. Debug lines are only written when the developer switch asks.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::paths;

const MAX_BYTES: u64 = 512 * 1024;

static NAME: Mutex<Option<&'static str>> = Mutex::new(None);
static VERBOSE: AtomicBool = AtomicBool::new(false);

/// Names this process's log (`gui`, `engine`) and installs the panic hook.
pub fn init(name: &'static str) {
    *NAME.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(name);
    set_verbose(crate::dev::DevSwitches::active().verbose_log);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        record_crash(name, &format!("{info}\n{backtrace}"));
        error(&format!("panic: {info}"));
        previous(info);
    }));
    info(&format!("{name} {} started", env!("CARGO_PKG_VERSION")));
}

pub fn set_verbose(on: bool) {
    VERBOSE.store(on, Ordering::Relaxed);
}

pub fn error(message: &str) {
    write("ERROR", message);
}
pub fn warn(message: &str) {
    write("WARN ", message);
}
pub fn info(message: &str) {
    write("INFO ", message);
}
pub fn debug(message: &str) {
    if VERBOSE.load(Ordering::Relaxed) {
        write("DEBUG", message);
    }
}

fn log_path(name: &str) -> std::path::PathBuf {
    paths::logs_dir().join(format!("{name}.log"))
}

fn write(level: &str, message: &str) {
    let Some(name) = *NAME.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) else { return };
    append(&log_path(name), &format!("{} {level} {message}\n", timestamp()));
}

fn append(path: &std::path::Path, line: &str) {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if path.metadata().is_ok_and(|meta| meta.len() > MAX_BYTES) {
        let _ = fs::rename(path, path.with_extension("log.1"));
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

fn record_crash(name: &str, details: &str) {
    append(&paths::logs_dir().join("crash.log"), &format!("---- {} {name} {} ----\n{details}\n", timestamp(), env!("CARGO_PKG_VERSION")));
}

/// The last `lines` lines of a log (`gui`, `engine`, `crash`), or a note.
pub fn tail(name: &str, lines: usize) -> String {
    match fs::read_to_string(log_path(name)) {
        Ok(text) => {
            let all: Vec<&str> = text.lines().collect();
            all[all.len().saturating_sub(lines)..].join("\n")
        }
        Err(_) => String::new(),
    }
}

/// Whether the previous run left a crash entry newer than `since` (unix s).
pub fn crashed_since(since: u64) -> bool {
    log_path("crash").metadata().and_then(|meta| meta.modified()).ok().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok()).is_some_and(|age| age.as_secs() > since)
}

pub fn clear(name: &str) {
    let _ = fs::remove_file(log_path(name));
    let _ = fs::remove_file(log_path(name).with_extension("log.1"));
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// "YYYY-MM-DD HH:MM:SS" in UTC (no date crate for one label).
pub fn format_unix(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let (hour, minute, second) = ((secs % 86_400) / 3600, (secs % 3600) / 60, secs % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
}

fn timestamp() -> String {
    format_unix(now_unix())
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats_time() {
        assert_eq!(super::format_unix(0), "1970-01-01 00:00:00");
        assert_eq!(super::format_unix(1_792_281_600), "2026-10-18 00:00:00");
    }
}
