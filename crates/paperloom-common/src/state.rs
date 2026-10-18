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
