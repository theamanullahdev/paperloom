//! Spawning helper programs (`ffmpeg`, the engine) the same way on every
//! platform.
//!
//! Two platform wrinkles live here so callers don't each rediscover them:
//! - Windows and macOS installs *bundle* their helpers next to the app's
//!   own executables (there's no system `ffmpeg` to rely on, and a macOS
//!   app started from Finder doesn't even get Homebrew's `PATH`), so a
//!   sibling of `current_exe()` wins over `PATH`.
//! - On Windows, starting a console program (ffmpeg) from a GUI app flashes
//!   a console window unless `CREATE_NO_WINDOW` is passed.

use std::path::PathBuf;
use std::process::Command;

/// `name` plus the platform's executable suffix (`.exe` on Windows).
pub fn exe_name(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// This program's executable. After a package upgrade replaced the file,
/// Linux reports the old one as "/usr/bin/paperloom-gui (deleted)": the
/// suffix is dropped so the path names the newly installed copy.
pub fn current_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let text = exe.to_string_lossy();
    Some(match text.strip_suffix(" (deleted)") {
        Some(path) => PathBuf::from(path),
        None => exe,
    })
}

/// Whether a newer copy of this program has been installed over the one
/// that's running (an `apt upgrade`, or the in-app updater). Linux only:
/// elsewhere the installer itself restarts the app. A `readlink`, cheap.
pub fn exe_replaced() -> bool {
    cfg!(target_os = "linux")
        && std::fs::read_link("/proc/self/exe").is_ok_and(|path| path.to_string_lossy().ends_with(" (deleted)"))
}

/// A binary shipped in the same directory as the running executable — true
/// during development (cargo puts every workspace binary in one
/// `target/<profile>/`), for the .deb/.rpm (`/usr/bin`), the Windows
/// install folder, and a macOS bundle's `Contents/MacOS`.
pub fn sibling_binary(name: &str) -> Option<PathBuf> {
    let exe = current_exe()?;
    let candidate = exe.parent()?.join(exe_name(name));
    candidate.is_file().then_some(candidate)
}

/// A `Command` for a helper program: the bundled copy if there is one,
/// otherwise whatever `PATH` resolves, with no console window on Windows.
pub fn command(name: &str) -> Command {
    let program = sibling_binary(name).unwrap_or_else(|| PathBuf::from(exe_name(name)));
    let mut command = Command::new(program);
    hide_console(&mut command);
    command
}

#[cfg(windows)]
pub fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn hide_console(_command: &mut Command) {}
