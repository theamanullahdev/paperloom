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
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::EngineNotFound => write!(f, "couldn't find or start paperloom-engine"),
            Error::Io(err) => write!(f, "{err}"),
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

/// Opens a full-screen preview window (`paperloom-engine --preview`). One
/// at a time: if the previous preview is still open, does nothing.
pub fn preview(path: &Path) -> Result<(), Error> {
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
    let child = command.arg("--preview").arg(path).spawn().map_err(Error::Io)?;
    *current = Some(child);
    Ok(())
}

fn send_or_spawn(command: &Command) -> Result<(), Error> {
    if ipc::send(command).is_ok() {
        return Ok(());
    }

    spawn_engine()?;

    // Give the daemon a moment to stand up its window and start listening
    // before giving up. This blocks the GUI's event loop briefly on a cold
    // start (acceptable here — same tradeoff as the blocking file-dialog
    // call elsewhere in this crate), but only the first time; the daemon
    // stays running after that.
    let mut last_error = None;
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(100));
        match ipc::send(command) {
            Ok(()) => return Ok(()),
            Err(err) => last_error = Some(err),
        }
    }

    Err(last_error.map(Error::Io).unwrap_or(Error::EngineNotFound))
}

/// The engine ships next to this executable in every layout (cargo's
/// `target/<profile>/`, `/usr/bin`, the Windows install folder, a macOS
/// bundle's `Contents/MacOS`) — see `paperloom_common::process`.
fn spawn_engine() -> Result<(), Error> {
    let engine_path = process::sibling_binary("paperloom-engine").ok_or(Error::EngineNotFound)?;
    let mut command = std::process::Command::new(engine_path);
    process::hide_console(&mut command);
    command.spawn().map(|_| ()).map_err(Error::Io)
}
