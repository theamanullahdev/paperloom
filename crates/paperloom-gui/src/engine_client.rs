//! Talks to the `paperloom-engine` daemon over its IPC socket
//! (`paperloom_common::ipc`). If the daemon isn't running yet, starts it —
//! the user shouldn't have to know a separate process exists.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use paperloom_common::ipc::{socket_path, Command};

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
    let mut stream = connect_or_spawn()?;
    stream.write_all(Command::Play(path.to_path_buf()).encode().as_bytes()).map_err(Error::Io)
}

/// Stops playback. Deliberately does *not* spawn the engine if it isn't
/// already running — there's nothing to stop in that case, so silently
/// succeeding is correct, not a bug swallowed.
pub fn stop() -> Result<(), Error> {
    let Ok(mut stream) = UnixStream::connect(socket_path()) else {
        return Ok(());
    };
    stream.write_all(Command::Stop.encode().as_bytes()).map_err(Error::Io)
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
    let engine = sibling_binary("paperloom-engine").ok_or(Error::EngineNotFound)?;
    let child = std::process::Command::new(engine).arg("--preview").arg(path).spawn().map_err(Error::Io)?;
    *current = Some(child);
    Ok(())
}

fn connect_or_spawn() -> Result<UnixStream, Error> {
    if let Ok(stream) = UnixStream::connect(socket_path()) {
        return Ok(stream);
    }

    spawn_engine()?;

    // Give the daemon a moment to stand up its window and bind the socket
    // before giving up. This blocks the GUI's event loop briefly on a cold
    // start (acceptable here — same tradeoff as the blocking file-dialog
    // call elsewhere in this crate), but only the first time; the daemon
    // stays running after that.
    for _ in 0..20 {
        if let Ok(stream) = UnixStream::connect(socket_path()) {
            return Ok(stream);
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    Err(Error::EngineNotFound)
}

fn spawn_engine() -> Result<(), Error> {
    let engine_path = sibling_binary("paperloom-engine").ok_or(Error::EngineNotFound)?;
    std::process::Command::new(engine_path).spawn().map(|_| ()).map_err(Error::Io)
}

/// Looks for a binary next to our own executable: true during development
/// (cargo puts every workspace binary in the same `target/debug/` dir) and
/// also true for a typical installed layout (same `bin/` directory) — no
/// packaging story exists yet, but this doesn't need to change when one
/// does, as long as both binaries still ship to the same directory.
fn sibling_binary(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let candidate = exe.parent()?.join(name);
    candidate.is_file().then_some(candidate)
}
