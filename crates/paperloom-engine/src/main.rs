//! Paperloom renderer/daemon.
//!
//! This is the always-on piece: it decides whether a wallpaper needs to be
//! rendered right now (per the pause triggers in PROJECT.md §8) and, when it
//! does, decodes video via libmpv and hands frames to the active desktop
//! backend (X11 root window or Wayland layer-shell).
//!
//! Both backends stay running as a daemon, listen on the IPC socket
//! (`paperloom_common::ipc`) for `paperloom-gui` to tell them what to
//! play, and poll the pause triggers (`pause`) to decide when to actually
//! render. The initial video path is an optional CLI arg either way,
//! useful for direct testing without the GUI. See `backends::x11::run`
//! and `backends::wayland::run` — the latter has never run against a real
//! compositor (no Wayland session exists in this dev environment); its
//! own doc comment explains exactly what that does and doesn't mean.

mod backends;
mod pause;

use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let path = env::args().nth(1);

    // WAYLAND_DISPLAY is the standard way an app detects a usable Wayland
    // session is available; falling back to X11 (directly, or via
    // XWayland) otherwise matches how every other Wayland-aware desktop
    // app picks its backend.
    let result = if env::var_os("WAYLAND_DISPLAY").is_some() {
        backends::wayland::run(path.as_deref())
    } else {
        backends::x11::run(path.as_deref())
    };

    if let Err(err) = result {
        eprintln!("paperloom-engine: {err}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
