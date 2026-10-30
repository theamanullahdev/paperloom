//! Developer switches: overrides for testing, stored in their own file
//! (`dev.toml`) and honoured only while Settings → Developer mode is on, so
//! they can never be left active by accident.

use std::fs;

use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DevSwitches {
    /// Decode on the CPU even if the setting says hardware.
    pub force_software: bool,
    /// Windows: use the most compatible video output straight away.
    pub safe_video: bool,
    /// Don't show the tray icon.
    pub disable_tray: bool,
    /// Write debug lines to the logs.
    pub verbose_log: bool,
    /// Never pause automatically (fullscreen app, lock, battery…).
    pub disable_auto_pause: bool,
    /// Windows: attach widgets to the desktop layer (under the icons) instead of plain windows
    /// directly above the desktop. Falls back to the plain windows if that isn't possible.
    pub widgets_under_icons: bool,
}

impl DevSwitches {
    pub fn load() -> Self {
        fs::read_to_string(paths::dev_file()).ok().and_then(|text| toml::from_str(&text).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = paths::dev_file();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, toml::to_string_pretty(self).expect("DevSwitches always serializes"))
    }

    /// The switches in force right now: all off unless developer mode is on.
    pub fn active() -> Self {
        if crate::load().developer_mode {
            Self::load()
        } else {
            Self::default()
        }
    }
}
