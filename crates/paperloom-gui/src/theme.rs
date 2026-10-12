//! Paperloom's own visual identity — see `DESIGN.md` for the full system
//! and where these values came from (the `ui-ux-pro-max` skill's design
//! database, not picked by eye). Warm, no blue/purple anywhere — a hard
//! constraint, not a preference.
//!
//! `Tokens` carries everything custom layout code (sidebar, top bar, cards)
//! needs directly; `from_mode` also derives the smaller `iced::Palette`
//! that built-in widgets (buttons, togglers, text inputs) style themselves
//! from automatically.

use iced::theme::Palette;
use iced::{Color, Font, Theme};
use paperloom_common::ThemeMode;

/// Body/UI text — everywhere by default (`main.rs` sets this as the
/// application's `default_font`). Its name matches the font file's own
/// "Regular" named instance exactly (verified against the TTF's `name`
/// table, not assumed) — unlike the display font below, there's no
/// variable-axis surprise here.
pub const BODY_FONT: Font = Font::with_name("IBM Plex Sans");

/// Page titles / big display text only — deliberately not applied
/// everywhere. This variable font's *default* instance (what you get
/// without explicitly setting weight/width/optical-size axis
/// coordinates) is ExtraBold at a large optical size, confirmed by
/// reading the TTF's `name` table directly, not assumed from the family
/// name alone. That's exactly right for a title, and exactly wrong for
/// body text, which is why it's scoped this narrowly.
pub const DISPLAY_FONT: Font = Font::with_name("Bricolage Grotesque");

#[derive(Debug, Clone, Copy)]
pub struct Tokens {
    pub background: Color,
    pub surface: Color,
    pub muted: Color,
    pub border: Color,
    pub foreground: Color,
    pub muted_foreground: Color,
    pub primary: Color,
    pub primary_hover: Color,
    pub success: Color,
    pub destructive: Color,
}

fn light() -> Tokens {
    Tokens {
        background: Color::from_rgb(1.0, 0.984, 0.921),        // #FFFBEB
        surface: Color::from_rgb(1.0, 1.0, 1.0),                // #FFFFFF
        muted: Color::from_rgb(0.973, 0.949, 0.941),            // #F8F2F0
        border: Color::from_rgb(0.949, 0.902, 0.886),           // #F2E6E2
        foreground: Color::from_rgb(0.110, 0.078, 0.063),       // #1C1410
        muted_foreground: Color::from_rgb(0.471, 0.443, 0.424), // #78716C
        primary: Color::from_rgb(0.604, 0.204, 0.071),          // #9A3412
        primary_hover: Color::from_rgb(0.761, 0.255, 0.047),    // #C2410C
        success: Color::from_rgb(0.020, 0.588, 0.412),          // #059669
        destructive: Color::from_rgb(0.863, 0.149, 0.149),      // #DC2626
    }
}

fn dark() -> Tokens {
    Tokens {
        background: Color::from_rgb(0.110, 0.098, 0.090),       // #1C1917
        surface: Color::from_rgb(0.149, 0.133, 0.125),          // #262220
        muted: Color::from_rgb(0.180, 0.157, 0.141),            // #2E2824
        border: Color::from_rgba(1.0, 1.0, 1.0, 0.08),
        foreground: Color::from_rgb(0.961, 0.937, 0.914),       // #F5EFE9
        muted_foreground: Color::from_rgb(0.659, 0.600, 0.557), // #A8998E
        primary: Color::from_rgb(0.984, 0.573, 0.235),          // #FB923C
        primary_hover: Color::from_rgb(0.992, 0.729, 0.455),    // #FDBA74
        success: Color::from_rgb(0.204, 0.827, 0.600),          // #34D399
        destructive: Color::from_rgb(0.973, 0.443, 0.443),      // #F87171
    }
}

pub fn tokens(mode: ThemeMode) -> Tokens {
    match mode {
        ThemeMode::Light => light(),
        ThemeMode::Dark | ThemeMode::System => dark(),
    }
}

/// `ThemeMode::System` doesn't attempt to detect the desktop's actual
/// light/dark preference (no simple portable way to do that from Iced
/// directly without another dependency) — falls back to dark.
pub fn from_mode(mode: ThemeMode) -> Theme {
    let t = tokens(mode);
    let name = match mode {
        ThemeMode::Light => "Paperloom Light",
        _ => "Paperloom Dark",
    };
    Theme::custom(
        name,
        Palette {
            background: t.background,
            text: t.foreground,
            primary: t.primary,
            success: t.success,
            warning: t.primary_hover,
            danger: t.destructive,
        },
    )
}
