//! Paperloom's own visual identity — not Iced's stock Light/Dark theme.
//!
//! Colors lean on macOS's system accent palette (the project's stated
//! visual reference point), since this is a from-scratch palette rather
//! than a value judgment about Iced's defaults.

use iced::theme::Palette;
use iced::{Color, Theme};
use paperloom_common::ThemeMode;

fn color(hex: u32) -> Color {
    let [r, g, b] = [(hex >> 16) & 0xFF, (hex >> 8) & 0xFF, hex & 0xFF].map(|c| c as f32 / 255.0);
    Color::from_rgb(r, g, b)
}

pub fn dark() -> Theme {
    Theme::custom(
        "Paperloom Dark",
        Palette {
            background: color(0x1C1C1F),
            text: color(0xF2F2F4),
            primary: color(0x0A84FF),
            success: color(0x32D74B),
            warning: color(0xFF9F0A),
            danger: color(0xFF453A),
        },
    )
}

pub fn light() -> Theme {
    Theme::custom(
        "Paperloom Light",
        Palette {
            background: color(0xF5F5F7),
            text: color(0x1D1D1F),
            primary: color(0x007AFF),
            success: color(0x30D158),
            warning: color(0xFF9500),
            danger: color(0xFF3B30),
        },
    )
}

/// `ThemeMode::System` doesn't attempt to detect the desktop's actual
/// light/dark preference (no simple portable way to do that from Iced
/// directly without another dependency) — falls back to dark, which fits
/// this app's use case (video wallpapers, typically used by people who
/// already lean toward dark UI chrome) better than assuming light.
pub fn from_mode(mode: ThemeMode) -> Theme {
    match mode {
        ThemeMode::Light => light(),
        ThemeMode::Dark | ThemeMode::System => dark(),
    }
}
