use std::fs;

use crate::{paths, Settings};

/// Loads settings from disk, falling back to defaults if the file is
/// missing or unreadable — a missing config file is the normal first-run
/// state, not an error.
pub fn load() -> Settings {
    fs::read_to_string(paths::config_file())
        .ok()
        .and_then(|contents| toml::from_str::<Settings>(&contents).ok())
        .unwrap_or_default()
        .with_builtin_sources()
}

pub fn save(settings: &Settings) -> std::io::Result<()> {
    let path = paths::config_file();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let contents = toml::to_string_pretty(settings).expect("Settings always serializes");
    fs::write(&path, contents)?;
    restrict_to_owner(&path)
}

/// The file can hold API keys: owner read/write only. On Windows the
/// per-user AppData folder it lives in is already private to the user.
#[cfg(unix)]
fn restrict_to_owner(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}
