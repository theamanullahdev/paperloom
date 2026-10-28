//! Desktop widgets: which ones are placed and where. Stored in
//! `widgets.toml`, written by the app and read by `paperloom-widgets`, the
//! separate process that draws them (the wallpaper engine never does).

use std::fs;

use serde::{Deserialize, Serialize};

use crate::paths;

/// The most widgets that can be placed at once.
pub const MAX_PLACED: usize = 24;

/// A widget Paperloom ships with.
pub struct Entry {
    pub kind: &'static str,
    pub name: &'static str,
    pub about: &'static str,
}

/// Every built-in widget. Having them listed costs nothing; only placed ones run.
pub const BUILT_IN: &[Entry] = &[
    Entry { kind: "clock", name: "Clock", about: "The time, with the date underneath." },
    Entry { kind: "date", name: "Date", about: "A calendar page: weekday, day, month and year." },
    Entry { kind: "cpu", name: "Processor", about: "How busy the processor is, across all cores." },
    Entry { kind: "memory", name: "Memory", about: "How much memory is in use." },
    Entry { kind: "disk", name: "Disk", about: "Used and free space on your main disk." },
    Entry { kind: "network", name: "Network", about: "Download and upload speed right now." },
    Entry { kind: "battery", name: "Battery", about: "Charge level, and whether you are plugged in." },
    Entry { kind: "uptime", name: "Uptime", about: "How long this computer has been running." },
];

pub fn entry(kind: &str) -> Option<&'static Entry> {
    BUILT_IN.iter().find(|entry| entry.kind == kind)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Widgets {
    /// Master switch (Settings → Widgets, and the tray menu).
    pub enabled: bool,
    pub placed: Vec<Placed>,
}

/// One widget on the desktop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Placed {
    pub id: u32,
    /// Which widget: `clock`, `date`, `cpu`, … or a pack id (`author/name`).
    pub kind: String,
    /// Which look draws it (`clock/neon`); empty = the kind's default look.
    pub skin: String,
    /// Top-left corner as a fraction of the monitor (0.0–1.0), so a
    /// resolution change keeps the widget where it was.
    pub x: f32,
    pub y: f32,
    /// 1.0 = the widget's natural size.
    pub scale: f32,
    /// 0.1–1.0.
    pub opacity: f32,
    /// The widget can be dragged on the desktop.
    pub drag: bool,
    /// Right-clicking the widget opens its menu.
    pub menu: bool,
}

impl Placed {
    /// It takes mouse input, so it sits above the desktop icons (the icons'
    /// window would otherwise swallow every click). With both off it is
    /// click-through and sits below them.
    pub fn interactive(&self) -> bool {
        self.drag || self.menu
    }
}

impl Default for Placed {
    fn default() -> Self {
        Self { id: 1, kind: "clock".into(), skin: String::new(), x: 0.76, y: 0.06, scale: 1.0, opacity: 1.0, drag: true, menu: true }
    }
}

impl Default for Widgets {
    fn default() -> Self {
        Self { enabled: false, placed: vec![Placed::default()] }
    }
}

impl Widgets {
    pub fn load() -> Self {
        let mut widgets: Self = fs::read_to_string(paths::widgets_file()).ok().and_then(|text| toml::from_str(&text).ok()).unwrap_or_default();
        widgets.placed.truncate(MAX_PLACED);
        for placed in &mut widgets.placed {
            placed.opacity = placed.opacity.clamp(0.1, 1.0);
            placed.scale = placed.scale.clamp(0.5, 3.0);
            placed.x = placed.x.clamp(0.0, 0.95);
            placed.y = placed.y.clamp(0.0, 0.95);
        }
        widgets
    }

    /// Places a new widget of `kind` a little below-left of the last one so
    /// it doesn't land exactly on top of it. `None` at the cap or for an unknown kind.
    pub fn add(&mut self, kind: &str) -> Option<u32> {
        if self.placed.len() >= MAX_PLACED || entry(kind).is_none() {
            return None;
        }
        let id = self.placed.iter().map(|placed| placed.id).max().unwrap_or(0) + 1;
        let step = (self.placed.len() % 8) as f32 * 0.04;
        self.placed.push(Placed { id, kind: kind.to_string(), x: (0.06 + step).min(0.9), y: (0.06 + step).min(0.9), ..Placed::default() });
        Some(id)
    }

    pub fn remove(&mut self, id: u32) {
        self.placed.retain(|placed| placed.id != id);
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = paths::widgets_file();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, toml::to_string_pretty(self).expect("Widgets always serializes"))
    }
}

/// When `widgets.toml` last changed; the widgets process reloads when this moves.
pub fn changed_at() -> Option<std::time::SystemTime> {
    fs::metadata(paths::widgets_file()).and_then(|meta| meta.modified()).ok()
}

/// Proof that this is the one running widgets process; released when dropped
/// or when the process ends for any reason (a crash included).
pub struct Running(#[allow(dead_code)] fs::File);

impl Running {
    /// `None` when another widgets process already holds it.
    pub fn claim() -> Option<Self> {
        Self::claim_at(&paths::widgets_lock())
    }

    fn claim_at(path: &std::path::Path) -> Option<Self> {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let file = fs::OpenOptions::new().create(true).truncate(false).write(true).open(path).ok()?;
        file.try_lock().ok().map(|()| Self(file))
    }
}

/// Switches widgets on or off (Settings, the tray, the Widgets page all
/// come here). Turning them on with nothing placed puts a clock on the
/// desktop, and starts the process; turning them off lets it end by itself.
pub fn set_enabled(enabled: bool) -> std::io::Result<()> {
    let mut widgets = Widgets::load();
    widgets.enabled = enabled;
    if enabled && widgets.placed.is_empty() {
        widgets.add("clock");
    }
    widgets.save()?;
    if enabled {
        start()?;
    }
    Ok(())
}

/// Starts the widgets process if none is running. It ends itself when
/// widgets are switched off or none is placed.
pub fn start() -> std::io::Result<()> {
    if is_running() {
        return Ok(());
    }
    let program = crate::process::sibling_binary("paperloom-widgets").ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "paperloom-widgets isn't installed next to Paperloom"))?;
    let mut command = std::process::Command::new(program);
    crate::process::hide_console(&mut command);
    command.stdin(std::process::Stdio::null()).spawn().map(|_| ())
}

/// Is a widgets process running right now?
pub fn is_running() -> bool {
    running_at(&paths::widgets_lock())
}

fn running_at(path: &std::path::Path) -> bool {
    let Ok(file) = fs::OpenOptions::new().create(true).truncate(false).write(true).open(path) else { return false };
    // Free means nobody holds it; the probe releases at once.
    file.try_lock().is_err()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_clamps() {
        let text = toml::to_string_pretty(&Widgets::default()).unwrap();
        let back: Widgets = toml::from_str(&text).unwrap();
        assert_eq!(back, Widgets::default());
        let wild: Widgets = toml::from_str("[[placed]]\nopacity = 0.0\nscale = 99.0\nx = 4.0").unwrap();
        assert_eq!(wild.placed.len(), 1);
    }

    #[test]
    fn adding_respects_the_cap_and_kinds() {
        let mut widgets = Widgets { enabled: true, placed: Vec::new() };
        assert!(widgets.add("nonsense").is_none());
        for _ in 0..MAX_PLACED {
            assert!(widgets.add("clock").is_some());
        }
        assert!(widgets.add("clock").is_none());
        let first = widgets.placed[0].id;
        widgets.remove(first);
        assert_eq!(widgets.placed.len(), MAX_PLACED - 1);
    }

    #[test]
    fn only_one_claims_the_lock() {
        let dir = std::env::temp_dir().join(format!("paperloom-widgets-lock-{}", std::process::id()));
        let lock = dir.join("widgets.lock");
        let first = Running::claim_at(&lock);
        assert!(first.is_some());
        assert!(Running::claim_at(&lock).is_none());
        assert!(running_at(&lock));
        drop(first);
        assert!(!running_at(&lock));
        let _ = fs::remove_dir_all(dir);
    }
}
