//! What every backend does with libmpv once it has a surface to draw on:
//! the wallpaper playback options, the IPC command handling, and
//! remembering what's playing so it survives a reboot. Backends only differ
//! in how they get a window (or GL context) onto the desktop and in how
//! they detect when to pause.

use std::error::Error;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use libmpv2::Mpv;
use paperloom_common::ipc::{Command, Listener};
use paperloom_common::EngineState;

/// Fail loudly on a bad CLI path before touching the display: it otherwise
/// shows up as a silent black screen (mpv accepts `loadfile` synchronously
/// and only fails to actually play asynchronously), which is confusing
/// when running the engine directly during development. Doesn't catch
/// every failure mode (corrupt file, unsupported codec) — only "no such file".
pub fn validate_cli_path(path: Option<&str>) -> Result<(), Box<dyn Error>> {
    match path {
        Some(path) if !Path::new(path).is_file() => Err(format!("no such video file: {path}").into()),
        _ => Ok(()),
    }
}

/// What to show at startup: the CLI path if one was given, otherwise
/// whatever was playing when the engine last ran (the login-autostart
/// case), as long as that file still exists.
pub fn startup_wallpaper(cli_path: Option<&str>) -> Option<String> {
    if let Some(path) = cli_path {
        return Some(path.to_string());
    }
    EngineState::load().current_wallpaper.filter(|path| path.is_file()).map(|path| path.to_string_lossy().into_owned())
}

/// Wallpaper defaults: loops forever, no on-screen controller, doesn't try
/// to grab keyboard/mouse input, and fills the screen like CSS
/// `background-size: cover` instead of letterboxing when the clip's aspect
/// ratio doesn't match the display's (`panscan=1.0` crops-to-fill).
pub fn configure_wallpaper(mpv: &Mpv) -> Result<(), Box<dyn Error>> {
    mpv.set_property("loop-file", "inf".to_string())?;
    mpv.set_property("osc", false)?;
    mpv.set_property("input-default-bindings", false)?;
    mpv.set_property("input-vo-keyboard", false)?;
    mpv.set_property("cursor-autohide", "no".to_string())?;
    mpv.set_property("panscan", 1.0)?;
    apply_mute_from_settings(mpv)?;
    Ok(())
}

fn apply_mute_from_settings(mpv: &Mpv) -> Result<(), Box<dyn Error>> {
    mpv.set_property("mute", paperloom_common::load().muted)?;
    Ok(())
}

/// Loads `path` and records it as the current wallpaper. Re-reads settings
/// first so a mute change takes effect on the next wallpaper without a
/// restart.
pub fn play(mpv: &Mpv, path: &str) {
    if let Err(err) = apply_mute_from_settings(mpv) {
        eprintln!("paperloom-engine: failed to apply settings: {err}");
    }
    if let Err(err) = mpv.command("loadfile", &[path]) {
        eprintln!("paperloom-engine: failed to play {path}: {err}");
        return;
    }
    remember(Some(path));
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        crate::desktop_background::show_frame_of_in_background(path);
        crate::tray::set_wallpaper(path);
    }
}

fn remember(path: Option<&str>) {
    // Load-modify-save: the state file also holds the saved desktop background.
    let mut state = EngineState::load();
    state.current_wallpaper = path.map(Into::into);
    if let Err(err) = state.save() {
        eprintln!("paperloom-engine: failed to save engine state: {err}");
    }
}

/// What the automatic triggers (fullscreen app, lock, battery…) want, and
/// whether the user paused from the tray. The wallpaper is paused if
/// either says so.
static AUTO_PAUSED: AtomicBool = AtomicBool::new(false);
static USER_PAUSED: AtomicBool = AtomicBool::new(false);

fn apply_pause(mpv: &Mpv) {
    let paused = AUTO_PAUSED.load(Ordering::Relaxed) || USER_PAUSED.load(Ordering::Relaxed);
    if let Err(err) = mpv.set_property("pause", paused) {
        eprintln!("paperloom-engine: failed to set pause state: {err}");
    }
}

/// Called by the backends with the automatic pause triggers' verdict.
pub fn set_paused(mpv: &Mpv, paused: bool) {
    AUTO_PAUSED.store(paused, Ordering::Relaxed);
    apply_pause(mpv);
}

/// Housekeeping on the backends' ~2s tick (with freshly loaded settings):
/// show or hide the tray icon to match the setting.
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(unused_variables))]
pub fn periodic(settings: &paperloom_common::Settings) {
    #[cfg(all(unix, not(target_os = "macos")))]
    crate::tray::sync(settings.show_tray_icon);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Quit,
}

/// Handles at most one pending IPC command. Call once per loop tick.
pub fn poll_ipc(listener: &mut Listener, mpv: &Mpv) -> Flow {
    let command = match listener.accept_command() {
        Ok(Some(command)) => command,
        Ok(None) => return Flow::Continue,
        Err(err) => {
            eprintln!("paperloom-engine: ipc error: {err}");
            return Flow::Continue;
        }
    };

    match command {
        Command::Play(path) => {
            if !path.is_file() {
                eprintln!("paperloom-engine: ipc Play for missing file: {}", path.display());
                return Flow::Continue;
            }
            play(mpv, &path.to_string_lossy());
        }
        // Stopping the wallpaper ends the engine: an idle engine would keep
        // its (now black) window over the desktop, and costs memory for
        // nothing. The GUI starts a fresh one on the next Play. The user's
        // own desktop background comes back too.
        Command::Stop => {
            let _ = mpv.command("stop", &[]);
            remember(None);
            #[cfg(all(unix, not(target_os = "macos")))]
            crate::desktop_background::restore();
            return Flow::Quit;
        }
        // Quit (before an update) keeps the current wallpaper remembered,
        // so the updated engine picks it back up.
        Command::Quit => return Flow::Quit,
        Command::Pause | Command::Resume => {
            let paused = command == Command::Pause;
            USER_PAUSED.store(paused, Ordering::Relaxed);
            apply_pause(mpv);
            #[cfg(all(unix, not(target_os = "macos")))]
            crate::tray::set_paused(paused);
        }
    }
    Flow::Continue
}
