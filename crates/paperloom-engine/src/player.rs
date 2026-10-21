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
        // A web address (the app's preview of an online wallpaper) streams.
        Some(path) if path.starts_with("http://") || path.starts_with("https://") => Ok(()),
        Some(path) if !Path::new(path).is_file() => Err(format!("no such video file: {path}").into()),
        _ => Ok(()),
    }
}

/// Remembers why the engine couldn't start (its console is hidden on
/// Windows) so the app can show the reason; cleared once playback works.
pub fn record_failure(reason: &dyn std::fmt::Display) {
    paperloom_common::log::error(&format!("engine failure: {reason}"));
    let log = paperloom_common::engine_log();
    if let Some(dir) = log.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(log, format!("{reason}\n"));
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn clear_failure() {
    let _ = std::fs::remove_file(paperloom_common::engine_log());
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

/// Cheaper-than-default settings for a looping background video: a small
/// demuxer cache (the default holds up to 150 MB of the file in memory),
/// no display-sync resampling, plain scaling, no subtitles, quiet logging.
/// Each is best-effort — an option an older libmpv doesn't know is skipped.
fn tune(mpv: &Mpv) {
    for (name, value) in TUNING {
        if name == "msg-level" {
            let _ = mpv.set_property(name, mpv_log_level().to_string());
            continue;
        }
        if let Err(err) = mpv.set_property(name, value.to_string()) {
            paperloom_common::log::debug(&format!("mpv option {name} skipped: {err}"));
        }
    }
}

const TUNING: [(&str, &str); 14] = [
        ("demuxer-max-bytes", "32MiB"),
        ("demuxer-max-back-bytes", "8MiB"),
        ("demuxer-readahead-secs", "3"),
        ("video-sync", "desync"),
        ("interpolation", "no"),
        ("scale", "bilinear"),
        ("cscale", "bilinear"),
        ("dscale", "bilinear"),
        ("correct-downscaling", "no"),
        ("sigmoid-upscaling", "no"),
        ("deband", "no"),
        ("dither-depth", "no"),
        ("sid", "no"),
        ("msg-level", "all=error"),
];

/// No audio decoder at all while muted; unmuting brings the track back.
fn set_audio(mpv: &Mpv, muted: bool) {
    let _ = mpv.set_property("aid", if muted { "no" } else { "auto" }.to_string());
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
    tune(mpv);
    apply_mute_from_settings(mpv)?;
    Ok(())
}

/// Settings that can change between wallpapers: audio, and hardware
/// decoding. `auto-safe` lets mpv use the GPU only through decoders it
/// trusts, falling back to software if none works — so turning this on
/// can't leave a black desktop on a machine that can't do it.
fn apply_mute_from_settings(mpv: &Mpv) -> Result<(), Box<dyn Error>> {
    let settings = paperloom_common::load();
    mpv.set_property("mute", settings.muted)?;
    set_audio(mpv, settings.muted);
    mpv.set_property("hwdec", if settings.hardware_decoding && !dev().force_software { "auto-safe" } else { "no" }.to_string())?;
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
    paperloom_common::log::info(&format!("playing {path}"));
    crate::perf_guard::reset();
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
    let (auto, user) = (AUTO_PAUSED.load(Ordering::Relaxed), USER_PAUSED.load(Ordering::Relaxed));
    if let Err(err) = mpv.set_property("pause", auto || user) {
        eprintln!("paperloom-engine: failed to set pause state: {err}");
    }
    publish_pause(auto, user);
}

/// Writes the pause state where the tray widget can read it — only when it
/// changed, so the ~2s tick doesn't touch the disk.
fn publish_pause(auto: bool, user: bool) {
    static LAST: std::sync::Mutex<Option<(bool, bool)>> = std::sync::Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if *last == Some((auto, user)) {
        return;
    }
    *last = Some((auto, user));
    let mut state = EngineState::load();
    (state.auto_paused, state.user_paused) = (auto, user);
    let _ = state.save();
}

/// Called by the backends with the automatic pause triggers' verdict.
pub fn set_paused(mpv: &Mpv, paused: bool) {
    AUTO_PAUSED.store(paused && !dev().disable_auto_pause, Ordering::Relaxed);
    apply_pause(mpv);
}

/// Set by `--selftest` so libmpv logs in detail.
pub static SELFTEST: AtomicBool = AtomicBool::new(false);

/// libmpv's log level for the current run.
pub fn mpv_log_level() -> &'static str {
    if SELFTEST.load(Ordering::Relaxed) {
        "all=info"
    } else if dev().verbose_log {
        "all=warn"
    } else {
        "all=error"
    }
}

/// Developer switches in force (all off unless developer mode is on).
static DEV: std::sync::Mutex<Option<paperloom_common::dev::DevSwitches>> = std::sync::Mutex::new(None);

pub fn dev() -> paperloom_common::dev::DevSwitches {
    DEV.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone().unwrap_or_default()
}

/// Re-reads the switches (only when developer mode is on; otherwise they
/// are cleared) — called at start and on the ~2s tick.
pub fn refresh_dev(settings: &paperloom_common::Settings) {
    let switches = if settings.developer_mode { paperloom_common::dev::DevSwitches::load() } else { Default::default() };
    paperloom_common::log::set_verbose(switches.verbose_log);
    *DEV.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(switches);
}

/// Housekeeping on the backends' ~2s tick (with freshly loaded settings):
/// the developer switches, the tray icon, and the engine's own status.
pub fn periodic(settings: &paperloom_common::Settings, mpv: &Mpv) {
    refresh_dev(settings);
    publish_status(mpv, settings.developer_mode);
    crate::rotation::tick(mpv);
    let paused = AUTO_PAUSED.load(Ordering::Relaxed) || USER_PAUSED.load(Ordering::Relaxed);
    let hardware = mpv.get_property::<String>("hwdec-current").is_ok_and(|name| !name.is_empty() && name != "no");
    crate::perf_guard::tick(mpv, settings.performance_guard, paused, hardware);
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut settings = settings.clone();
        settings.show_tray_icon &= !dev().disable_tray;
        crate::tray::sync(&settings);
    }
}

/// Tells the rest of the app what the engine and its video look like
/// (decoder in use, codec, size, frame rate…). Written only when something
/// changed; the property reads are a few microseconds.
fn publish_status(mpv: &Mpv, with_drops: bool) {
    use paperloom_common::EngineInfo;
    static LAST: std::sync::Mutex<Option<EngineInfo>> = std::sync::Mutex::new(None);
    static STARTED: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    let started = *STARTED.get_or_init(paperloom_common::log::now_unix);

    let text = |name: &str| mpv.get_property::<String>(name).unwrap_or_default();
    let info = EngineInfo {
        pid: std::process::id(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        started,
        hwdec: text("hwdec-current"),
        codec: text("video-codec"),
        width: mpv.get_property::<i64>("width").unwrap_or(0),
        height: mpv.get_property::<i64>("height").unwrap_or(0),
        fps: mpv.get_property::<f64>("container-fps").unwrap_or(0.0),
        video_output: text("current-vo"),
        dropped_frames: if with_drops { mpv.get_property::<i64>("frame-drop-count").unwrap_or(0) } else { 0 },
        guard_level: crate::perf_guard::level(),
    };
    let mut last = LAST.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if last.as_ref() == Some(&info) {
        return;
    }
    let mut state = EngineState::load();
    state.engine = info.clone();
    let _ = state.save();
    *last = Some(info);
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
        Command::Mute(muted) => {
            let _ = mpv.set_property("mute", muted);
            set_audio(mpv, muted);
        }
        Command::Ping => {}
        Command::Next => crate::rotation::next(mpv),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every tuning option must be one this libmpv accepts, or the tuning
    /// would silently do nothing.
    #[test]
    fn tuning_options_are_known_to_libmpv() {
        let mpv = Mpv::with_initializer(|init| {
            init.set_property("vo", "null")?;
            init.set_property("ao", "null")?;
            Ok(())
        })
        .expect("libmpv starts headless");
        let rejected: Vec<_> = TUNING.iter().filter(|(name, value)| mpv.set_property(name, value.to_string()).is_err()).collect();
        assert!(rejected.is_empty(), "libmpv rejected: {rejected:?}");
        for (name, value) in [("vd-lavc-fast", "yes"), ("vd-lavc-skiploopfilter", "nonref"), ("vd-lavc-skipframe", "nonref"), ("aid", "no"), ("aid", "auto")] {
            assert!(mpv.set_property(name, value.to_string()).is_ok(), "{name}={value}");
        }
    }
}
