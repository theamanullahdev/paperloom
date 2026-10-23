//! What the engine is currently playing, persisted so it comes back after a
//! reboot (the engine is started at login with no arguments — see
//! `autostart`). Written by the engine itself, the one process that knows
//! for certain what's on screen; kept out of `Settings` so the GUI and the
//! engine never race to write the same file.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineState {
    pub current_wallpaper: Option<PathBuf>,
    /// The desktop's own background settings from before Paperloom
    /// replaced them with a still of the video (Linux: so the workspace
    /// switcher/overview, which draw the system background, match).
    /// Restored when the wallpaper is stopped.
    pub saved_background: Vec<SavedSetting>,
    /// Paused from the tray / widget (held until Resume).
    pub user_paused: bool,
    /// Paused by an automatic trigger (fullscreen app, lock, battery…).
    pub auto_paused: bool,
    /// What the engine itself reports about the running engine and the video
    /// it is playing (read by Usage, the Developers page and diagnostics).
    pub engine: EngineInfo,
}

/// Written by the engine, only when a value changes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineInfo {
    pub pid: u32,
    pub version: String,
    /// Unix seconds when this engine process started.
    pub started: u64,
    /// mpv's `hwdec-current`: "no" or empty means software decoding.
    pub hwdec: String,
    pub codec: String,
    pub width: i64,
    pub height: i64,
    pub fps: f64,
    pub video_output: String,
    /// Frames dropped so far (kept up to date only in developer mode).
    pub dropped_frames: i64,
    /// 0 = full quality; 1–2 = the performance guard lowered decoding quality.
    pub guard_level: u8,
    /// Whether each global shortcut could be registered: "" (none set),
    /// "ok" or "in use" (another program already owns that key).
    pub hotkey_next: String,
    pub hotkey_pause: String,
}

/// One `gsettings` value: schema, key, and the value exactly as
/// `gsettings get` printed it (a GVariant literal, so `set` takes it back).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedSetting {
    pub schema: String,
    pub key: String,
    pub value: String,
}

impl EngineState {
    pub fn load() -> Self {
        fs::read_to_string(paths::state_file()).ok().and_then(|contents| toml::from_str(&contents).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = paths::state_file();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, toml::to_string_pretty(self).expect("EngineState always serializes"))
    }
}
