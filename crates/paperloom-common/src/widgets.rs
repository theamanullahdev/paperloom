//! Desktop widgets: which ones are placed and where. Stored in
//! `widgets.toml`, written by the app and read by `paperloom-widgets`, the
//! separate process that draws them (the wallpaper engine never does).

use std::fs;

use serde::{Deserialize, Serialize};

use crate::paths;

/// The most widgets that can be placed at once.
pub const MAX_PLACED: usize = 24;

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
    /// Top-left corner as a fraction of the monitor (0.0–1.0), so a
    /// resolution change keeps the widget where it was.
    pub x: f32,
    pub y: f32,
    /// 1.0 = the widget's natural size.
    pub scale: f32,
    /// 0.1–1.0.
    pub opacity: f32,
    /// Mouse clicks pass through to whatever is underneath.
    pub click_through: bool,
}

impl Default for Placed {
    fn default() -> Self {
        Self { id: 1, kind: "clock".into(), x: 0.76, y: 0.06, scale: 1.0, opacity: 1.0, click_through: true }
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

    pub fn save(&self) -> std::io::Result<()> {
        let path = paths::widgets_file();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, toml::to_string_pretty(self).expect("Widgets always serializes"))
    }
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
}
