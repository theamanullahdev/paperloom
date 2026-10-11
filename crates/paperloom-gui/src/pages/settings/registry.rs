//! The searchable catalog of settings.
//!
//! Every setting the app exposes needs an entry here with aliases a user
//! might actually type — e.g. searching "dark mode" must surface the Theme
//! setting, not just the literal word "theme". This is what makes the
//! settings search in PROJECT.md §6 real instead of a plain label filter.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingId {
    PauseFullscreenApp,
    PauseScreenLock,
    PauseBatterySaver,
    PauseWindowCovering,
    ImportMode,
    Theme,
    Muted,
}

pub struct Entry {
    pub id: SettingId,
    pub label: &'static str,
    pub section: &'static str,
    aliases: &'static [&'static str],
}

pub const ENTRIES: &[Entry] = &[
    Entry {
        id: SettingId::Theme,
        label: "Theme",
        section: "Appearance",
        aliases: &["dark mode", "light mode", "dark theme", "light theme", "appearance"],
    },
    Entry {
        id: SettingId::PauseFullscreenApp,
        label: "Pause when a fullscreen app is focused",
        section: "Performance & idle behavior",
        aliases: &["fullscreen", "performance", "idle", "optimizer"],
    },
    Entry {
        id: SettingId::PauseScreenLock,
        label: "Pause when the screen is locked",
        section: "Performance & idle behavior",
        aliases: &["lock screen", "locked", "performance", "idle", "optimizer"],
    },
    Entry {
        id: SettingId::PauseBatterySaver,
        label: "Pause on battery saver",
        section: "Performance & idle behavior",
        aliases: &["battery", "power saver", "performance", "idle", "optimizer"],
    },
    Entry {
        id: SettingId::PauseWindowCovering,
        label: "Pause when a window covers the desktop",
        section: "Performance & idle behavior",
        aliases: &["maximized", "covered", "performance", "idle", "optimizer"],
    },
    Entry {
        id: SettingId::ImportMode,
        label: "Wallpaper import mode",
        section: "Storage",
        aliases: &["storage", "library", "copy", "optimize", "import", "re-encode"],
    },
    Entry {
        id: SettingId::Muted,
        label: "Mute wallpaper audio",
        section: "Playback",
        aliases: &["sound", "audio", "volume", "unmute", "silent"],
    },
];

/// Case-insensitive match against the label, section, and every alias.
pub fn matches(entry: &Entry, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return false;
    }
    entry.label.to_lowercase().contains(&query)
        || entry.section.to_lowercase().contains(&query)
        || entry
            .aliases
            .iter()
            .any(|alias| alias.to_lowercase().contains(&query))
}
