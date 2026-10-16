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

/// One online wallpaper source in the user's list. Built-in sources keep a
/// fixed `id` (their domain); user-added ones get `custom-<n>` ids.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceSetting {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// Empty for sources that don't need one. Stored in the config file
    /// (made owner-readable only), not a keyring — see `config_store::save`.
    pub api_key: String,
    /// Page URL for a user-added source ("{q}" is replaced with the search
    /// text if present); unused for built-ins.
    pub url: String,
    pub builtin: bool,
}

pub const SOURCE_MOTIONBGS: &str = "motionbgs.com";
pub const SOURCE_PEXELS: &str = "pexels.com";
pub const SOURCE_PIXABAY: &str = "pixabay.com";

impl SourceSetting {
    /// Whether this source can be used yet — Pexels/Pixabay refuse to
    /// answer without a key.
    pub fn needs_key(&self) -> bool {
        matches!(self.id.as_str(), SOURCE_PEXELS | SOURCE_PIXABAY)
    }
}

fn builtin_sources() -> Vec<SourceSetting> {
    let make = |id: &str, name: &str, enabled: bool| SourceSetting {
        id: id.to_string(),
        name: name.to_string(),
        enabled,
        api_key: String::new(),
        url: String::new(),
        builtin: true,
    };
    vec![
        make(SOURCE_MOTIONBGS, "MotionBGs", true),
        // Off until the user adds a key, so a fresh install doesn't show
        // "needs API key" errors for sources they never asked for.
        make(SOURCE_PEXELS, "Pexels (videos)", false),
        make(SOURCE_PIXABAY, "Pixabay (videos)", false),
    ]
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
    pub pause_on_low_battery: bool,
    pub pause_on_window_covering_desktop: bool,
    pub import_mode: ImportMode,
    pub theme: ThemeMode,
    /// Whether wallpaper playback is muted. Defaults to muted — matches
    /// every other live-wallpaper tool's convention (Wallpaper Engine,
    /// Lively, mpvpaper), since audio from a background wallpaper is
    /// usually unwanted.
    pub muted: bool,
    pub sources: Vec<SourceSetting>,
}

impl Settings {
    /// Adds any built-in source missing from `sources` (a config written
    /// before that source existed). Never touches existing entries.
    pub fn with_builtin_sources(mut self) -> Self {
        for builtin in builtin_sources() {
            if !self.sources.iter().any(|source| source.id == builtin.id) {
                self.sources.push(builtin);
            }
        }
        self
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            pause_on_fullscreen_app: true,
            pause_on_screen_lock: true,
            pause_on_low_battery: true,
            pause_on_window_covering_desktop: true,
            import_mode: ImportMode::default(),
            theme: ThemeMode::default(),
            muted: true,
            sources: builtin_sources(),
        }
    }
}
