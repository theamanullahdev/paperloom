use std::path::PathBuf;

use directories::ProjectDirs;

fn project_dirs() -> ProjectDirs {
    ProjectDirs::from("", "", "paperloom").expect("could not resolve a home directory for this user")
}

/// Where `Settings` is persisted.
pub fn config_file() -> PathBuf {
    project_dirs().config_dir().join("config.toml")
}

/// What the engine was last playing (`EngineState`).
pub fn state_file() -> PathBuf {
    project_dirs().data_dir().join("engine-state.toml")
}

/// Why the engine last failed to start (its console is hidden on Windows,
/// so errors would otherwise vanish).
pub fn engine_log() -> PathBuf {
    project_dirs().data_dir().join("engine.log")
}

/// Where the app's own logs live (`gui.log`, `engine.log`-style rotating
/// files, `crash.log`).
pub fn logs_dir() -> PathBuf {
    project_dirs().data_dir().join("logs")
}

/// Developer switches (see `dev.rs`); a separate file so they are never
/// mixed into the user's settings.
pub fn dev_file() -> PathBuf {
    project_dirs().config_dir().join("dev.toml")
}

/// The engine's recent CPU/memory readings (see `usage_history.rs`).
pub fn usage_history_file() -> PathBuf {
    project_dirs().data_dir().join("engine-usage.txt")
}

/// Installed widget looks (one folder each, with `skin.toml` and its fonts and images).
pub fn skins_dir() -> PathBuf {
    project_dirs().data_dir().join("skins")
}

/// Placed desktop widgets (see `widgets.rs`).
pub fn widgets_file() -> PathBuf {
    project_dirs().config_dir().join("widgets.toml")
}

/// Held (exclusively) by the running widgets process; see `widgets.rs`.
pub fn widgets_lock() -> PathBuf {
    project_dirs().config_dir().join("widgets.lock")
}

/// Installed stacks of widgets (one folder each, with `stack.toml`).
pub fn stacks_dir() -> PathBuf {
    project_dirs().data_dir().join("stacks")
}

/// What the widgets process is doing right now (see `widgets.rs`); the developer page reads it.
pub fn widgets_status() -> PathBuf {
    project_dirs().data_dir().join("widgets-status.toml")
}

/// Rotation and schedule (see `rotation.rs`).
pub fn rotation_file() -> PathBuf {
    project_dirs().config_dir().join("rotation.toml")
}

/// Still frames the engine hands to the desktop as its background.
pub fn background_frames_dir() -> PathBuf {
    project_dirs().data_dir().join("background")
}

/// Downloaded installers for app updates (see `paperloom-fetch::update`).
pub fn updates_dir() -> PathBuf {
    project_dirs().cache_dir().join("updates")
}

/// The app-managed library folder imported wallpapers are copied into
/// (unless `ImportMode::ReferenceInPlace` is in use for a given file).
pub fn library_dir() -> PathBuf {
    project_dirs().data_dir().join("library")
}

/// Where imported wallpapers are copied: the folder chosen in Settings, or
/// `library_dir()`. (The manifest itself always stays in `library_dir()`.)
pub fn wallpapers_dir() -> PathBuf {
    crate::load().library_folder.filter(|dir| !dir.as_os_str().is_empty()).unwrap_or_else(library_dir)
}

/// Generated preview-frame thumbnails for the library gallery. Separate
/// from `library_dir()` so a `ReferenceInPlace` entry (whose source file
/// lives elsewhere and isn't ours to touch) still gets a thumbnail we own
/// and can freely delete.
pub fn thumbnails_dir() -> PathBuf {
    project_dirs().data_dir().join("thumbnails")
}

/// In-progress and finished downloads from the online library, plus the
/// queue manifest. Kept apart from `library_dir()` so a half-finished
/// `.part` file can never show up as an importable wallpaper.
pub fn downloads_dir() -> PathBuf {
    project_dirs().data_dir().join("downloads")
}

/// Cached preview images for online-library browsing (disposable — safe to
/// delete at any time, they're re-fetched on demand).
pub fn online_cache_dir() -> PathBuf {
    project_dirs().cache_dir().join("online")
}
