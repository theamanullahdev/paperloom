//! Paperloom renderer/daemon.
//!
//! This is the always-on piece: it decides whether a wallpaper needs to be
//! rendered right now (per the pause triggers in PROJECT.md §8) and, when it
//! does, decodes video via libmpv and hands frames to the active desktop
//! backend:
//!
//! - Linux: X11 desktop-type window, or a Wayland layer-shell surface.
//! - Windows: a window parented behind the desktop icons (`WorkerW`).
//! - macOS: a borderless window at the desktop window level.
//!
//! Every backend stays running as a daemon, listens on the IPC channel
//! (`paperloom_common::ipc`) for `paperloom-gui` to tell it what to play,
//! and polls its pause triggers to decide when to actually render. The
//! initial video path is an optional CLI arg (useful for direct testing
//! without the GUI); without one, the last wallpaper is restored. Only the
//! X11 backend has been run on a real desktop so far — see each backend's
//! own doc comment for what has and hasn't been verified.

// No console window when the GUI (or the login autostart) launches us.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod backends;
#[cfg(all(unix, not(target_os = "macos")))]
mod desktop_background;
#[cfg(all(unix, not(target_os = "macos")))]
mod pause;
mod player;

use std::env;
use std::error::Error;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let first = args.next();

    // `--preview <file>`: a one-off full-screen window for the GUI's
    // double-click preview, not the wallpaper daemon.
    let result = if first.as_deref() == Some("--preview") {
        match args.next() {
            Some(file) => preview(&file),
            None => Err("--preview needs a file".into()),
        }
    } else {
        run(first.as_deref())
    };

    if let Err(err) = result {
        eprintln!("paperloom-engine: {err}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

#[cfg(all(unix, not(target_os = "macos")))]
fn run(path: Option<&str>) -> Result<(), Box<dyn Error>> {
    // WAYLAND_DISPLAY is the standard way an app detects a usable Wayland
    // session is available; falling back to X11 (directly, or via
    // XWayland) otherwise matches how every other Wayland-aware desktop
    // app picks its backend.
    if env::var_os("WAYLAND_DISPLAY").is_some() {
        backends::wayland::run(path)
    } else {
        backends::x11::run(path)
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn preview(path: &str) -> Result<(), Box<dyn Error>> {
    backends::preview::run(path)
}

#[cfg(windows)]
fn run(path: Option<&str>) -> Result<(), Box<dyn Error>> {
    backends::windows::run(path)
}

#[cfg(windows)]
fn preview(path: &str) -> Result<(), Box<dyn Error>> {
    backends::windows::preview(path)
}

#[cfg(target_os = "macos")]
fn run(path: Option<&str>) -> Result<(), Box<dyn Error>> {
    backends::macos::run(path)
}

#[cfg(target_os = "macos")]
fn preview(path: &str) -> Result<(), Box<dyn Error>> {
    backends::macos::preview(path)
}
