//! Talks to the `paperloom-engine` daemon over its IPC channel
//! (`paperloom_common::ipc` — a Unix socket, or a named pipe on Windows). If the daemon isn't running yet, starts it —
//! the user shouldn't have to know a separate process exists.

use std::path::Path;
use std::time::Duration;

use paperloom_common::ipc::{self, Command};
use paperloom_common::process;

#[derive(Debug)]
pub enum Error {
    EngineNotFound,
    Io(std::io::Error),
    /// The engine started but exited straight away; carries what it said.
    EngineFailed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::EngineNotFound => write!(f, "couldn't find or start paperloom-engine"),
            Error::Io(err) => write!(f, "{err}"),
            Error::EngineFailed(why) => write!(f, "the wallpaper engine stopped at startup: {why}"),
        }
    }
}

pub fn play(path: &Path) -> Result<(), Error> {
    send_or_spawn(&Command::Play(path.to_path_buf()))
}

/// Stops playback. Deliberately does *not* spawn the engine if it isn't
/// already running — there's nothing to stop in that case, so silently
/// succeeding is correct, not a bug swallowed.
pub fn stop() -> Result<(), Error> {
    let _ = ipc::send(&Command::Stop);
    Ok(())
}

/// Asks a running engine to exit (before an update replaces its binary).
/// Not running is fine.
pub fn quit() {
    let _ = ipc::send(&Command::Quit);
}

/// Restarts a running engine so it runs the newly installed binary: asks
/// it to quit (it keeps the remembered wallpaper), waits until it's gone,
/// and starts the new one, which picks the wallpaper back up. Not running
/// is fine: nothing to restart.
pub fn restart_engine() {
    if ipc::send(&Command::Quit).is_err() {
        return;
    }
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(100));
        if ipc::send(&Command::Quit).is_err() {
            break;
        }
    }
    let _ = spawn_engine();
}

/// Restarts Paperloom into the newly installed version: the engine, then a
/// fresh window, then this process exits.
pub fn restart_app() -> ! {
    restart_engine();
    if let Some(exe) = process::current_exe() {
        let _ = std::process::Command::new(exe).spawn();
    }
    std::process::exit(0)
}

/// Opens a full-screen preview window (`paperloom-engine --preview`). One
/// at a time: if the previous preview is still open, does nothing.
pub fn preview(path: &Path) -> Result<(), Error> {
    preview_target(&path.to_string_lossy())
}

/// Same, for a web address: mpv streams it.
pub fn preview_url(url: &str) -> Result<(), Error> {
    preview_target(url)
}

fn preview_target(target: &str) -> Result<(), Error> {
    use std::sync::Mutex;
    static CURRENT: Mutex<Option<std::process::Child>> = Mutex::new(None);

    let mut current = CURRENT.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(child) = current.as_mut() {
        if child.try_wait().map_err(Error::Io)?.is_none() {
            return Ok(());
        }
    }
    let engine = process::sibling_binary("paperloom-engine").ok_or(Error::EngineNotFound)?;
    let mut command = std::process::Command::new(engine);
    process::hide_console(&mut command);
    let child = command.arg("--preview").arg(target).spawn().map_err(Error::Io)?;
    *current = Some(child);
    Ok(())
}

/// How long a cold start may take. Windows is slow the first time: the
/// antivirus scans the big libmpv DLL before the engine can run at all.
const STARTUP_WAIT: Duration = if cfg!(windows) { Duration::from_secs(45) } else { Duration::from_secs(5) };

fn send_or_spawn(command: &Command) -> Result<(), Error> {
    if ipc::send(command).is_ok() {
        // A running engine that couldn't set up the desktop says why.
        return match std::fs::read_to_string(paperloom_common::engine_log()) {
            Ok(why) if !why.trim().is_empty() => Err(Error::EngineFailed(why.trim().to_string())),
            _ => Ok(()),
        };
    }

    let _ = std::fs::remove_file(paperloom_common::engine_log());
    let mut child = spawn_engine()?;

    // Wait for the daemon to stand up and listen — but stop as soon as it
    // has exited, with the reason. Callers run this off the UI thread.
    let started = std::time::Instant::now();
    while started.elapsed() < STARTUP_WAIT {
        std::thread::sleep(Duration::from_millis(100));
        if ipc::send(command).is_ok() {
            return Ok(());
        }
        if let Ok(Some(status)) = child.try_wait() {
            let log = std::fs::read_to_string(paperloom_common::engine_log()).unwrap_or_default();
            return Err(Error::EngineFailed(explain_exit(status.code(), log.trim())));
        }
    }
    let log = std::fs::read_to_string(paperloom_common::engine_log()).unwrap_or_default();
    Err(Error::EngineFailed(if log.trim().is_empty() {
        format!("it didn't start within {} seconds — your antivirus may be scanning it; try again in a minute", STARTUP_WAIT.as_secs())
    } else {
        log.trim().to_string()
    }))
}

/// Turns the engine's exit into something readable. Windows reports
/// loader failures as NTSTATUS codes.
fn explain_exit(code: Option<i32>, log: &str) -> String {
    if !log.is_empty() {
        return log.to_string();
    }
    let Some(code) = code else { return "it stopped right after starting".into() };
    let hint = match code as u32 {
        0xC000_0135 => "a required DLL is missing — reinstall Paperloom",
        0xC000_007B => "a DLL has the wrong architecture — reinstall Paperloom",
        0xC000_0142 => "a DLL failed to initialise — reinstall Paperloom or update your graphics driver",
        0xC000_0005 => "access violation — blocked by antivirus or a graphics driver crash",
        _ => "it stopped right after starting",
    };
    format!("{hint} (exit code 0x{:08X})", code as u32)
}

/// The engine ships next to this executable in every layout (cargo's
/// `target/<profile>/`, `/usr/bin`, the Windows install folder, a macOS
/// bundle's `Contents/MacOS`) — see `paperloom_common::process`.
fn spawn_engine() -> Result<std::process::Child, Error> {
    let engine_path = process::sibling_binary("paperloom-engine").ok_or(Error::EngineNotFound)?;
    if cfg!(windows) {
        let dll = engine_path.with_file_name("libmpv-2.dll");
        if !dll.is_file() {
            return Err(Error::EngineFailed("libmpv-2.dll is missing from the Paperloom folder — reinstall Paperloom".into()));
        }
    }
    let mut command = std::process::Command::new(engine_path);
    process::hide_console(&mut command);
    command.spawn().map_err(Error::Io)
}
