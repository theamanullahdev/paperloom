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
    PauseLowBattery,
    PauseWindowCovering,
    ImportMode,
    Theme,
    Muted,
    HardwareDecoding,
    LowBatteryPercent,
    Autostart,
    Updates,
    AllowMature,
    TrayIcon,
    TrayStyle,
    TraySize,
    LibraryFolder,
    DeveloperMode,
    PerformanceGuard,
    PauseApps,
    PauseIdle,
    Hotkeys,
    HiddenPassword,
    ScreenTarget,
    LockScreen,
}

pub struct Entry {
    pub id: SettingId,
    pub label: &'static str,
    pub section: &'static str,
    aliases: &'static [&'static str],
}

pub const ENTRIES: &[Entry] = &[
    Entry {
        id: SettingId::LibraryFolder,
        label: "Wallpaper folder",
        section: "Library",
        aliases: &["storage", "folder", "location", "disk", "drive", "where wallpapers are saved", "save location", "library path"],
    },
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
        id: SettingId::PauseLowBattery,
        label: "Pause on low battery",
        section: "Performance & idle behavior",
        aliases: &["battery", "low battery", "power saver", "performance", "idle", "optimizer"],
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
    Entry {
        id: SettingId::HardwareDecoding,
        label: "Use the graphics card to play video",
        section: "Playback",
        aliases: &["hardware decoding", "hwdec", "gpu", "cpu usage", "performance", "battery", "vaapi", "nvdec", "cooler", "fan"],
    },
    Entry {
        id: SettingId::LowBatteryPercent,
        label: "Low battery level",
        section: "Performance & idle behavior",
        aliases: &["battery", "threshold", "percent", "power saver", "low battery"],
    },
    Entry {
        id: SettingId::Autostart,
        label: "Start Paperloom when I log in",
        section: "Startup",
        aliases: &["autostart", "login", "boot", "startup", "reboot", "launch at login", "run at startup"],
    },
    Entry {
        id: SettingId::AllowMature,
        label: "Allow 18+ content in online search",
        section: "Content",
        aliases: &["18+", "nsfw", "adult", "mature", "safe search", "safesearch", "explicit", "filter"],
    },
    Entry {
        id: SettingId::TrayIcon,
        label: "Show Paperloom in the system tray",
        section: "Startup",
        aliases: &["tray", "system tray", "applet", "panel", "indicator", "notification area", "icon"],
    },
    Entry {
        id: SettingId::TrayStyle,
        label: "Tray icon style",
        section: "Startup",
        aliases: &["tray color", "colored", "monochrome", "black and white", "white icon", "black icon", "applet style", "symbolic", "match theme", "system theme icon", "adaptive"],
    },
    Entry {
        id: SettingId::TraySize,
        label: "Tray icon size",
        section: "Startup",
        aliases: &["tray size", "icon size", "applet size", "bigger icon", "smaller icon"],
    },
    Entry {
        id: SettingId::LockScreen,
        label: "Show a still of the wallpaper on the lock screen",
        section: "Playback",
        aliases: &["lock screen", "login screen", "sign in", "still image", "static", "background picture"],
    },
    Entry {
        id: SettingId::ScreenTarget,
        label: "Which screen shows the wallpaper",
        section: "Playback",
        aliases: &["monitor", "display", "primary screen", "second screen", "secondary", "both screens", "multi monitor", "dual monitor", "extend"],
    },
    Entry {
        id: SettingId::HiddenPassword,
        label: "Password for the Hidden section",
        section: "Privacy",
        aliases: &["password", "lock", "hidden", "private", "pin", "protect", "privacy"],
    },
    Entry {
        id: SettingId::Hotkeys,
        label: "Keyboard shortcuts",
        section: "Playback",
        aliases: &["hotkey", "shortcut", "next wallpaper", "global key", "keybinding", "keys", "pause key"],
    },
    Entry {
        id: SettingId::PauseApps,
        label: "Pause while certain apps are focused",
        section: "Performance & idle behavior",
        aliases: &["game", "games", "steam", "obs", "zoom", "app list", "pause for app", "performance"],
    },
    Entry {
        id: SettingId::PauseIdle,
        label: "Pause when I'm away",
        section: "Performance & idle behavior",
        aliases: &["idle", "inactive", "away", "no input", "afk", "screensaver", "save power", "battery"],
    },
    Entry {
        id: SettingId::PerformanceGuard,
        label: "Keep CPU use low automatically",
        section: "Playback",
        aliases: &["performance guard", "cpu", "fan", "hot", "laptop", "lower quality", "battery", "throttle"],
    },
    Entry {
        id: SettingId::DeveloperMode,
        label: "Developer mode",
        section: "Advanced",
        aliases: &["developer", "dev", "debug", "diagnostics", "logs", "advanced", "testing", "support"],
    },
    Entry {
        id: SettingId::Updates,
        label: "Updates",
        section: "About",
        aliases: &["update", "upgrade", "new version", "release", "version", "changelog", "about"],
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
