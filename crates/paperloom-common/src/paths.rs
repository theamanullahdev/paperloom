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
