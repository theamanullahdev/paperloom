use std::path::PathBuf;

use directories::ProjectDirs;

fn project_dirs() -> ProjectDirs {
    ProjectDirs::from("", "", "paperloom").expect("could not resolve a home directory for this user")
}

/// Where `Settings` is persisted.
pub fn config_file() -> PathBuf {
    project_dirs().config_dir().join("config.toml")
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
