use std::fs;

use crate::{paths, Settings};

/// Loads settings from disk, falling back to defaults if the file is
/// missing or unreadable — a missing config file is the normal first-run
/// state, not an error.
pub fn load() -> Settings {
    fs::read_to_string(paths::config_file())
        .ok()
        .and_then(|contents| toml::from_str(&contents).ok())
        .unwrap_or_default()
}

pub fn save(settings: &Settings) -> std::io::Result<()> {
    let path = paths::config_file();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let contents = toml::to_string_pretty(settings).expect("Settings always serializes");
    fs::write(path, contents)
}
