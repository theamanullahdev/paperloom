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

/// Makes sure an engine is running (starting one if needed).
pub fn start() -> Result<(), Error> {
    send_or_spawn(&Command::Ping)
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
    // The widgets process is a program file too: an installer can't replace it while it runs.
    end_widgets();
}

/// Ends this copy's widgets process, if one runs. Settings are untouched, so
/// `paperloom_common::widgets::start()` brings the same widgets back.
pub fn end_widgets() {
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let ended = own_processes(&system, "paperloom-widgets").into_iter().fold(false, |ended, process| process.kill() | ended);
    if ended {
        std::thread::sleep(Duration::from_millis(300));
    }
}

/// Restarts the widgets process into the newly installed binary, if it was running.
pub fn restart_widgets() {
    let was_running = paperloom_common::widgets::is_running();
    end_widgets();
    if was_running {
        let _ = paperloom_common::widgets::start();
    }
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
    // One that ignored the request (stuck, or an older build) is ended
    // outright, so the new engine isn't started next to it.
    end_stale_engines();
    let _ = spawn_engine();
}

/// The wallpaper engines that belong to *this* copy of Paperloom: the same
/// program file (an upgraded one shows as "(deleted)") run by the same user,
/// never a full-screen preview. Another install or profile on the machine is
/// never touched.
fn own_engines(system: &sysinfo::System) -> Vec<&sysinfo::Process> {
    own_processes(system, "paperloom-engine")
}

/// The same, for any of Paperloom's own helper programs.
fn own_processes<'a>(system: &'a sysinfo::System, program: &str) -> Vec<&'a sysinfo::Process> {
    let Some(mine) = process::sibling_binary(program) else { return Vec::new() };
    let me = sysinfo::get_current_pid().ok().and_then(|pid| system.process(pid)).and_then(|process| process.user_id());
    system
        .processes()
        .values()
        .filter(|process| process.exe().is_some_and(|exe| same_program(exe, &mine)))
        .filter(|process| me.is_none() || process.user_id() == me)
        .filter(|process| !process.cmd().iter().any(|arg| arg == "--preview"))
        .filter(|process| process.pid().as_u32() != std::process::id())
        .collect()
}

/// Whether two paths are the same program, ignoring the " (deleted)" Linux
/// adds to a file that was replaced while it ran.
fn same_program(running: &Path, installed: &Path) -> bool {
    let running = running.to_string_lossy();
    let running = running.strip_suffix(" (deleted)").unwrap_or(&running);
    let installed = installed.to_string_lossy();
    if cfg!(windows) {
        running.eq_ignore_ascii_case(&installed)
    } else {
        running == installed
    }
}

/// At start-up: if this copy's wallpaper engines are piled up, or one that
/// has been running a while never reported in (a stuck one from an older
/// build), end them and start a single fresh one. Healthy setups are left alone.
pub fn heal_engines() {
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let engines = own_engines(&system);
    if engines.is_empty() {
        return;
    }
    // Only judge engines that have had time to report their status.
    if engines.iter().any(|process| process.run_time() < 20) {
        return;
    }
    let reported = paperloom_common::EngineState::load().engine.pid;
    let healthy = engines.len() == 1 && engines[0].pid().as_u32() == reported;
    if !healthy {
        paperloom_common::log::warn(&format!("{} wallpaper engine(s) running, none reporting in; restarting one", engines.len()));
        end_stale_engines();
        let _ = spawn_engine();
    }
}

/// Ends every wallpaper engine of this copy still running.
fn end_stale_engines() {
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let ended = own_engines(&system).into_iter().fold(false, |ended, process| process.kill() | ended);
    if ended {
        std::thread::sleep(Duration::from_millis(400));
    }
}

/// Restarts Paperloom into the newly installed version: the engine, then a
/// fresh window, then this process exits.
pub fn restart_app() -> ! {
    // Let go of the "one window" socket first: otherwise the new window finds
    // this one still listening, hands it a request and quits, leaving nothing.
    crate::instance::release("gui");
    if let Some(exe) = process::current_exe() {
        let _ = std::process::Command::new(exe).spawn();
    }
    // The new window comes up before anything is closed, so there is never a
    // moment with no Paperloom window for the desktop to hand focus around in,
    // and the wallpaper and widgets are swapped while the new window is showing.
    crate::instance::wait_for_replacement("gui", Duration::from_secs(12));
    restart_engine();
    restart_widgets();
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
    command.arg("--on-demand");
    process::hide_console(&mut command);
    command.spawn().map_err(Error::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_upgraded_engine_is_still_the_same_program() {
        let installed = Path::new("/usr/bin/paperloom-engine");
        assert!(same_program(Path::new("/usr/bin/paperloom-engine"), installed));
        assert!(same_program(Path::new("/usr/bin/paperloom-engine (deleted)"), installed));
    }

    #[test]
    fn another_install_is_not_ours() {
        let installed = Path::new("/usr/bin/paperloom-engine");
        assert!(!same_program(Path::new("/opt/other/paperloom-engine"), installed));
        assert!(!same_program(Path::new("/media/x/target/debug/paperloom-engine"), installed));
    }
}
