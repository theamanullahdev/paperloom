//! The desktop's own colors, for the tray widget when Paperloom's theme is
//! set to "System": the accent, background and text colors of the user's
//! GTK theme (Mint-Y, Adwaita, Colloid, …), read from the theme's CSS.
//! Only the colors are borrowed — the widget keeps Paperloom's layout.
//! Returns `None` when nothing usable is found; the caller then uses
//! Paperloom's own theme.

use std::path::PathBuf;
use std::process::Command;

use iced::Color;

use crate::theme::Tokens;

pub fn tokens(ours: Tokens) -> Option<Tokens> {
    let css = theme_css()?;
    let background = color(&css, "theme_bg_color")?;
    let foreground = color(&css, "theme_fg_color")?;
    let accent = color(&css, "theme_selected_bg_color")?;
    let mix = |a: Color, b: Color, amount: f32| Color::from_rgb(a.r + (b.r - a.r) * amount, a.g + (b.g - a.g) * amount, a.b + (b.b - a.b) * amount);
    Some(Tokens {
        background,
        surface: mix(background, foreground, 0.05),
        muted: mix(background, foreground, 0.10),
        border: Color { a: 0.12, ..foreground },
        foreground,
        muted_foreground: mix(foreground, background, 0.38),
        primary: accent,
        primary_hover: mix(accent, foreground, 0.2),
        success: ours.success,
        destructive: ours.destructive,
    })
}

fn gsettings(schema: &str, key: &str) -> Option<String> {
    let output = Command::new("gsettings").args(["get", schema, key]).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().trim_matches('\'').to_string())
}

/// The user's GTK theme stylesheet, as text (the dark variant when the
/// desktop asks for dark and the theme ships one).
fn theme_css() -> Option<String> {
    let name = ["org.cinnamon.desktop.interface", "org.gnome.desktop.interface", "org.mate.interface"]
        .iter()
        .find_map(|schema| gsettings(schema, "gtk-theme"))
        .filter(|name| !name.is_empty())?;
    let dark = gsettings("org.gnome.desktop.interface", "color-scheme").is_some_and(|scheme| scheme == "prefer-dark");
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(home) = &home {
        roots.push(home.join(".themes"));
        roots.push(home.join(".local/share/themes"));
    }
    roots.push("/usr/share/themes".into());
    for root in roots {
        let dir = root.join(&name).join("gtk-3.0");
        let files = if dark { ["gtk-dark.css", "gtk.css"] } else { ["gtk.css", "gtk-dark.css"] };
        for file in files {
            if let Ok(css) = std::fs::read_to_string(dir.join(file)) {
                return Some(css);
            }
        }
    }
    None
}

/// `@define-color <name> <value>;` — `#rrggbb`, `rgb()/rgba()` or an
/// `@other_name` reference.
fn color(css: &str, name: &str) -> Option<Color> {
    color_at(css, name, 0)
}

fn parse(css: &str, value: &str, depth: u8) -> Option<Color> {
    if let Some(reference) = value.strip_prefix('@') {
        return if depth < 4 { color_at(css, reference, depth + 1) } else { None };
    }
    if let Some(hex) = value.strip_prefix('#') {
        let digits = match hex.len() {
            3 => hex.chars().flat_map(|c| [c, c]).collect::<String>(),
            6 => hex.to_string(),
            _ => return None,
        };
        let channel = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok().map(|value| f32::from(value) / 255.0);
        return Some(Color::from_rgb(channel(0)?, channel(2)?, channel(4)?));
    }
    let inner = value.strip_prefix("rgba(").or_else(|| value.strip_prefix("rgb("))?.strip_suffix(')')?;
    let parts: Vec<f32> = inner.split(',').filter_map(|part| part.trim().parse().ok()).collect();
    (parts.len() >= 3).then(|| Color::from_rgb(parts[0] / 255.0, parts[1] / 255.0, parts[2] / 255.0))
}

fn color_at(css: &str, name: &str, depth: u8) -> Option<Color> {
    let prefix = format!("@define-color {name} ");
    let value = css.lines().find_map(|line| line.trim().strip_prefix(&prefix))?.trim().trim_end_matches(';').trim();
    parse(css, value, depth)
}
