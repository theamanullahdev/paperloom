//! What is left of the engine after "Stop": no window, no decoder, no
//! wallpaper, only the tray icon, so the icon never disappears and the app
//! can be started again from it.
//!
//! It listens for the next `PLAY` (handing over to a fresh engine, which
//! brings the icon back itself) or `QUIT`, and checks the tray setting every
//! couple of seconds. Between those it sleeps, so it costs next to nothing.

use std::time::{Duration, Instant};

use paperloom_common::ipc::{Command, Listener};

const TICK: Duration = Duration::from_millis(250);
const SETTINGS_EVERY: Duration = Duration::from_secs(2);

/// `paperloom-engine --idle`.
pub fn run() {
    let settings = paperloom_common::load();
    // Nothing to keep: no icon wanted, so the engine simply ends.
    if !settings.show_tray_icon {
        return;
    }
    let Ok(mut listener) = Listener::bind() else { return };
    crate::tray::sync(&settings);
    paperloom_common::log::info("wallpaper stopped; keeping the tray icon");

    let mut last = Instant::now();
    loop {
        match listener.accept_command() {
            Ok(Some(Command::Play(path))) => {
                hand_over(listener, &path);
                return;
            }
            Ok(Some(Command::Quit | Command::Stop)) => break,
            _ => {}
        }
        if last.elapsed() >= SETTINGS_EVERY {
            last = Instant::now();
            let settings = paperloom_common::load();
            if !settings.show_tray_icon {
                break;
            }
            crate::tray::sync(&settings);
        }
        std::thread::sleep(TICK);
    }
    crate::tray::shutdown();
}

/// Something is to play again: a new engine takes over (it makes the icon,
/// the window and the decoder), and this one ends.
fn hand_over(listener: Listener, path: &std::path::Path) {
    crate::tray::shutdown();
    drop(listener);
    let Some(engine) = paperloom_common::process::sibling_binary("paperloom-engine").or_else(paperloom_common::process::current_exe) else { return };
    let mut command = std::process::Command::new(engine);
    paperloom_common::process::hide_console(&mut command);
    if let Err(err) = command.arg("--on-demand").arg(path).stdin(std::process::Stdio::null()).spawn() {
        paperloom_common::log::error(&format!("couldn't start the engine again: {err}"));
    }
}

/// Called by the engine that just stopped: starts a fresh, tiny process for
/// the icon (this one still holds the decoder's memory and libraries), then
/// lets go of its own icon. The new icon is up before the old one goes.
pub fn relaunch() {
    let Some(engine) = paperloom_common::process::sibling_binary("paperloom-engine").or_else(paperloom_common::process::current_exe) else { return };
    let mut command = std::process::Command::new(engine);
    paperloom_common::process::hide_console(&mut command);
    if command.arg("--idle").stdin(std::process::Stdio::null()).spawn().is_ok() {
        // Time for the new process to put its icon up before this one's goes.
        std::thread::sleep(Duration::from_millis(900));
    }
    crate::tray::shutdown();
}
