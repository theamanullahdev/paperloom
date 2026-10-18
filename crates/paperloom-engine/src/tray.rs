//! Paperloom's system-tray icon (Linux), via the StatusNotifierItem D-Bus
//! protocol that Cinnamon's panel (XApp status applet), KDE, XFCE, Budgie
//! and GNOME's AppIndicator extension all show.
//!
//! It lives in the engine because that's the process that runs while a
//! wallpaper plays, whether or not the Paperloom window is open — so the
//! icon means "a wallpaper is running". Its menu talks to the engine over
//! the same IPC channel the GUI uses (`PAUSE`/`RESUME`/`STOP`), so the tray
//! thread never touches mpv itself.
//!
//! Started and stopped from the engine's periodic tick (`sync`) following
//! `Settings::show_tray_icon`. Best-effort: no tray host on the desktop
//! means no icon, never an error.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::StandardItem;
use ksni::{Icon, MenuItem, ToolTip, Tray};
use paperloom_common::ipc::{self, Command};
use paperloom_common::{TraySize, TrayStyle};

struct PaperloomTray {
    paused: bool,
    wallpaper: Option<String>,
    style: TrayStyle,
    size: TraySize,
    /// For `TrayStyle::Auto`: whether the panel is dark (→ white icon).
    dark_panel: bool,
}

static HANDLE: Mutex<Option<Handle<PaperloomTray>>> = Mutex::new(None);
/// Starting the tray failed outright (no D-Bus session, …): don't retry on
/// every tick, only once the setting is switched off and on again.
static FAILED: AtomicBool = AtomicBool::new(false);
/// `TrayStyle::Auto`'s last look at the desktop theme: (when, dark).
static PANEL: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
const PANEL_RECHECK: Duration = Duration::from_secs(10);

/// Shows or hides the icon, and restyles it, to match the settings (cheap
/// when nothing changed).
pub fn sync(settings: &paperloom_common::Settings) {
    let (show, style, size) = (settings.show_tray_icon, settings.tray_style, settings.tray_size);
    // Only `Auto` looks at the theme (it shells out to `gsettings`).
    let dark_panel = if show && style == TrayStyle::Auto { panel_dark() } else { true };
    let mut handle = HANDLE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if !show {
        FAILED.store(false, Ordering::Relaxed);
    }
    match (show, handle.is_some()) {
        (true, false) if !FAILED.load(Ordering::Relaxed) => {
            let wallpaper = paperloom_common::EngineState::load()
                .current_wallpaper
                .and_then(|path| path.file_stem().map(|stem| stem.to_string_lossy().into_owned()));
            let tray = PaperloomTray { paused: false, wallpaper, style, size, dark_panel };
            // Inside Flatpak the sandbox can't own the spec's well-known name.
            let sandboxed = std::env::var_os("FLATPAK_ID").is_some();
            // `assume_sni_available`: publish the item even if no tray host
            // (StatusNotifierWatcher) is up yet — at login the engine often
            // starts before the panel — and register as soon as one appears.
            match tray.disable_dbus_name(sandboxed).assume_sni_available(true).spawn() {
                Ok(spawned) => *handle = Some(spawned),
                Err(err) => {
                    FAILED.store(true, Ordering::Relaxed);
                    eprintln!("paperloom-engine: no tray icon: {err}");
                }
            }
        }
        (false, true) => {
            if let Some(old) = handle.take() {
                old.shutdown().wait();
            }
        }
        (true, true) => {
            if let Some(handle) = handle.as_ref() {
                // ksni compares before/after and only re-sends what changed.
                handle.update(|tray| {
                    tray.style = style;
                    tray.size = size;
                    tray.dark_panel = dark_panel;
                });
            }
        }
        _ => {}
    }
}

/// Keeps the icon's menu/tooltip in step with the engine.
pub fn set_paused(paused: bool) {
    update(|tray| tray.paused = paused);
}

pub fn set_wallpaper(path: &str) {
    let name = std::path::Path::new(path).file_stem().map(|stem| stem.to_string_lossy().into_owned());
    update(|tray| tray.wallpaper = name);
}

fn update(f: impl FnOnce(&mut PaperloomTray)) {
    if let Some(handle) = HANDLE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_ref() {
        handle.update(f);
    }
}

/// Whether the desktop's panel is dark, rechecked every `PANEL_RECHECK`.
fn panel_dark() -> bool {
    let mut cached = PANEL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    match *cached {
        Some((at, dark)) if at.elapsed() < PANEL_RECHECK => dark,
        _ => {
            let dark = detect_dark_panel();
            *cached = Some((Instant::now(), dark));
            dark
        }
    }
}

/// Best guess at the panel's shade from the desktop's own theme setting;
/// dark when unsure, since most panels are.
fn detect_dark_panel() -> bool {
    let gsettings = |schema: &str, key: &str| {
        let output = std::process::Command::new("gsettings").args(["get", schema, key]).output().ok()?;
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().trim_matches('\'').to_lowercase())
    };
    let shade = |name: &str| {
        if name.contains("dark") || name.contains("black") || name.contains("nokto") {
            Some(true)
        } else if name.contains("light") || name.contains("white") {
            Some(false)
        } else {
            None
        }
    };
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();

    if desktop.contains("cinnamon") {
        // The panel follows the Cinnamon ("desktop") theme, not the app theme.
        if let Some(name) = gsettings("org.cinnamon.theme", "name") {
            // Mint-Y / Mint-X / Mint-L without "Dark" have a light panel; the
            // stock "cinnamon" theme a dark one.
            return shade(&name).unwrap_or(!name.starts_with("mint-"));
        }
    }
    if desktop.contains("kde") {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| std::path::Path::new(&home).join(".config")));
        if let Some(globals) = config.and_then(|dir| std::fs::read_to_string(dir.join("kdeglobals")).ok()) {
            if let Some(scheme) = globals.lines().find_map(|line| line.strip_prefix("ColorScheme=")) {
                return shade(&scheme.to_lowercase()).unwrap_or(false);
            }
        }
        return false; // Breeze: light panel.
    }
    if desktop.contains("gnome") {
        // GNOME Shell's top bar is dark unless light mode is asked for.
        return gsettings("org.gnome.desktop.interface", "color-scheme").as_deref() != Some("prefer-light");
    }
    if gsettings("org.gnome.desktop.interface", "color-scheme").as_deref() == Some("prefer-dark") {
        return true;
    }
    gsettings("org.gnome.desktop.interface", "gtk-theme").and_then(|name| shade(&name)).unwrap_or(true)
}

fn send(command: Command) {
    if let Err(err) = ipc::send(&command) {
        eprintln!("paperloom-engine: tray couldn't reach the engine: {err}");
    }
}

fn open_paperloom() {
    if let Err(err) = paperloom_common::process::command("paperloom-gui").spawn() {
        eprintln!("paperloom-engine: couldn't open Paperloom: {err}");
    }
}

fn open_tray_settings() {
    if let Err(err) = paperloom_common::process::command("paperloom-gui").arg("--tray-settings").spawn() {
        eprintln!("paperloom-engine: couldn't open Paperloom: {err}");
    }
}

/// Pre-rendered ARGB32 icons (`assets/tray/<art>-<fill>-<px>.argb`: big-endian
/// width and height, then pixels), generated from the logo: the colored
/// icon and a white outline glyph (black and `Auto` are derived from it), each filling 70/85/100% of the canvas,
/// at the pixel sizes panels ask for. Embedded, so the icon shows even when
/// the desktop's icon theme doesn't have Paperloom's.
macro_rules! tray_set {
    ($art:literal, $fill:literal) => {
        [
            include_bytes!(concat!("../assets/tray/", $art, "-", $fill, "-22.argb")).as_slice(),
            include_bytes!(concat!("../assets/tray/", $art, "-", $fill, "-32.argb")).as_slice(),
            include_bytes!(concat!("../assets/tray/", $art, "-", $fill, "-48.argb")).as_slice(),
            include_bytes!(concat!("../assets/tray/", $art, "-", $fill, "-64.argb")).as_slice(),
        ]
    };
}

fn pixmaps(style: TrayStyle, size: TraySize, dark_panel: bool) -> Vec<Icon> {
    let style = match style {
        TrayStyle::Auto if dark_panel => TrayStyle::White,
        TrayStyle::Auto => TrayStyle::Black,
        other => other,
    };
    let files: [&[u8]; 4] = match (style, size) {
        (TrayStyle::Colored, TraySize::Small) => tray_set!("colored", "s"),
        (TrayStyle::Colored, TraySize::Medium) => tray_set!("colored", "m"),
        (TrayStyle::Colored, TraySize::Large) => tray_set!("colored", "l"),
        (_, TraySize::Small) => tray_set!("glyph", "s"),
        (_, TraySize::Medium) => tray_set!("glyph", "m"),
        (_, TraySize::Large) => tray_set!("glyph", "l"),
    };
    files
        .iter()
        .map(|file| {
            let size = |at: usize| i32::from_be_bytes([file[at], file[at + 1], file[at + 2], file[at + 3]]);
            let mut data = file[8..].to_vec();
            if style == TrayStyle::Black {
                // The glyph is white; black keeps its alpha, zeroes R, G, B.
                for pixel in data.as_chunks_mut::<4>().0 {
                    pixel[1..].fill(0);
                }
            }
            Icon { width: size(0), height: size(4), data }
        })
        .collect()
}

impl Tray for PaperloomTray {
    /// Any click opens the menu ("Open Paperloom" is its first item).
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        "paperloom".into()
    }

    fn title(&self) -> String {
        "Paperloom".into()
    }

    /// No theme icon name: most panels prefer a name over pixmaps, and the
    /// style/size options only exist as pixmaps.
    fn icon_name(&self) -> String {
        String::new()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        pixmaps(self.style, self.size, self.dark_panel)
    }

    fn tool_tip(&self) -> ToolTip {
        let description = match (&self.wallpaper, self.paused) {
            (Some(name), false) => format!("Playing {name}"),
            (Some(name), true) => format!("Paused: {name}"),
            (None, _) => "No wallpaper playing".into(),
        };
        ToolTip { title: "Paperloom".into(), description, icon_name: "paperloom".into(), icon_pixmap: Vec::new() }
    }

    /// The tray host went away (panel restarted): keep the item alive; it
    /// re-registers when the host comes back (`watcher_online`).
    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        true
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let now_playing = match &self.wallpaper {
            Some(name) => format!("{} {name}", if self.paused { "Paused:" } else { "Playing:" }),
            None => "No wallpaper playing".into(),
        };
        vec![
            StandardItem { label: now_playing, enabled: false, ..Default::default() }.into(),
            MenuItem::Separator,
            StandardItem { label: "Open Paperloom".into(), icon_name: "paperloom".into(), activate: Box::new(|_| open_paperloom()), ..Default::default() }
                .into(),
            StandardItem {
                label: if self.paused { "Resume wallpaper".into() } else { "Pause wallpaper".into() },
                icon_name: if self.paused { "media-playback-start".into() } else { "media-playback-pause".into() },
                activate: Box::new(|tray: &mut Self| {
                    tray.paused = !tray.paused;
                    send(if tray.paused { Command::Pause } else { Command::Resume });
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Stop wallpaper".into(),
                icon_name: "media-playback-stop".into(),
                activate: Box::new(|_| send(Command::Stop)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Tray icon settings…".into(),
                icon_name: "preferences-system".into(),
                activate: Box::new(|_| open_tray_settings()),
                ..Default::default()
            }
            .into(),
        ]
    }
}
