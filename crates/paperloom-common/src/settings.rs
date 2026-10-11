use serde::{Deserialize, Serialize};

/// How a video file gets added to the library when the user imports it.
///
/// See PROJECT.md §7 — this is deliberately a setting, not a mandate, because
/// each mode trades disk usage against the guarantees the app can make about
/// the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ImportMode {
    /// Copy into the managed library and re-encode for efficient playback.
    ///
    /// Re-encoding isn't implemented yet (needs the engine's decode
    /// pipeline), so this currently behaves like `CopyNoReencode` until
    /// that lands.
    #[default]
    CopyAndOptimize,
    /// Copy into the managed library as-is, no transcoding.
    CopyNoReencode,
    /// Don't copy — just remember the path the user gave us.
    ReferenceInPlace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl std::fmt::Display for ImportMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ImportMode::CopyAndOptimize => "Copy & optimize",
            ImportMode::CopyNoReencode => "Copy, no re-encode",
            ImportMode::ReferenceInPlace => "Reference in place",
        })
    }
}

impl std::fmt::Display for ThemeMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ThemeMode::System => "System",
            ThemeMode::Light => "Light",
            ThemeMode::Dark => "Dark",
        })
    }
}

/// User-configurable behavior, persisted to `paths::config_file()`.
///
/// Every field here is something the user can change — nothing about
/// behavior should be hardcoded elsewhere. New settings go here first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub pause_on_fullscreen_app: bool,
    pub pause_on_screen_lock: bool,
    pub pause_on_battery_saver: bool,
    pub pause_on_window_covering_desktop: bool,
    pub import_mode: ImportMode,
    pub theme: ThemeMode,
    /// Whether wallpaper playback is muted. Defaults to muted — matches
    /// every other live-wallpaper tool's convention (Wallpaper Engine,
    /// Lively, mpvpaper), since audio from a background wallpaper is
    /// usually unwanted.
    pub muted: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            pause_on_fullscreen_app: true,
            pause_on_screen_lock: true,
            pause_on_battery_saver: true,
            pause_on_window_covering_desktop: true,
            import_mode: ImportMode::default(),
            theme: ThemeMode::default(),
            muted: true,
        }
    }
}
