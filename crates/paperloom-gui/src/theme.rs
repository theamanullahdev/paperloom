//! The theme the window is drawn with.
//!
//! Themes are files (`paperloom-theme`: format, checker, built-ins). This
//! module holds the one in use, turned into what iced draws with: colour
//! tokens for the light and the dark mode, corner shapes, fonts, part
//! overrides, the backdrop and picture layers. The built-in "Paperloom"
//! theme is the look the app has always had (see `DESIGN.md`).
//!
//! Pages keep taking a `Tokens` (colours) as before; shape, fonts and part
//! styles are read through the small functions here, so a page never
//! hard-codes a radius or a font.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use iced::theme::Palette;
use iced::widget::image::Handle;
use iced::{Color, Font, Shadow, Theme, Vector};
use paperloom_common::ThemeMode;
use paperloom_theme::{Layer, Layout, Listed, Rgba, Shape};

const DEFAULT_BODY: &str = "IBM Plex Sans";
const DEFAULT_DISPLAY: &str = "Bricolage Grotesque";

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

fn color(value: Rgba) -> Color {
    Color::from_rgba(value[0], value[1], value[2], value[3])
}

fn tokens_of(palette: &paperloom_theme::Palette) -> Tokens {
    Tokens {
        background: color(palette.background),
        surface: color(palette.surface),
        muted: color(palette.muted),
        border: color(palette.border),
        foreground: color(palette.foreground),
        muted_foreground: color(palette.muted_foreground),
        primary: color(palette.primary),
        primary_hover: color(palette.primary_hover),
        success: color(palette.success),
        destructive: color(palette.destructive),
    }
}

/// One named part's overrides (see `paperloom_theme::PARTS`).
#[derive(Debug, Clone, Copy, Default)]
pub struct PartStyle {
    pub background: Option<Color>,
    pub foreground: Option<Color>,
    pub border: Option<Color>,
    pub border_width: Option<f32>,
    pub radius: Option<f32>,
}

/// A picture layer, loaded.
#[derive(Debug, Clone)]
pub struct LayerPictures {
    pub spec: Layer,
    pub frames: Vec<Handle>,
}

/// The theme in use, ready to draw with.
#[derive(Debug)]
pub struct Active {
    pub listed: Listed,
    dark: Tokens,
    light: Tokens,
    dark_extra: (Color, Color),
    light_extra: (Color, Color),
    pub shape: Shape,
    pub layout: Layout,
    body: Font,
    display: Font,
    /// The body family as written (changing it needs a restart: iced takes
    /// its default font once, at start).
    pub body_family: String,
    parts: HashMap<String, PartStyle>,
    pub gradient: Vec<Color>,
    pub backdrop: Option<Handle>,
    pub layers: Vec<LayerPictures>,
    /// Font files to hand to iced (`iced::font::load`).
    pub font_files: Vec<PathBuf>,
}

impl Active {
    pub fn id(&self) -> &str {
        &self.listed.theme.id
    }

    pub fn spec(&self) -> &paperloom_theme::Theme {
        &self.listed.theme
    }

    /// Any layer with more than one frame (the app ticks only then).
    pub fn animated(&self) -> Option<f32> {
        self.layers.iter().filter(|layer| layer.frames.len() > 1).map(|layer| layer.spec.fps.clamp(1.0, 30.0)).fold(None, |fastest: Option<f32>, fps| Some(fastest.map_or(fps, |f| f.max(fps))))
    }
}

/// Font families are `&'static str` in iced; each distinct name is kept once.
pub fn family(name: &str) -> &'static str {
    static NAMES: RwLock<Vec<&'static str>> = RwLock::new(Vec::new());
    if let Some(known) = NAMES.read().unwrap_or_else(|poisoned| poisoned.into_inner()).iter().find(|known| **known == name) {
        return known;
    }
    let kept: &'static str = Box::leak(name.to_string().into_boxed_str());
    NAMES.write().unwrap_or_else(|poisoned| poisoned.into_inner()).push(kept);
    kept
}

fn build(listed: Listed) -> Active {
    let theme = &listed.theme;
    let dir = listed.dir().map(PathBuf::from);
    let file = |relative: &str| dir.as_ref().map(|dir| dir.join(relative)).filter(|path| path.is_file());
    let (dark, light) = (theme.palette(true), theme.palette(false));
    let font = |written: &str, default: &'static str| Font::with_name(if written.trim().is_empty() { default } else { family(written.trim()) });
    let parts = theme
        .part
        .iter()
        .map(|(name, part)| {
            let parse = |written: &Option<String>| written.as_deref().and_then(paperloom_theme::parse_color).map(color);
            (name.clone(), PartStyle { background: parse(&part.background), foreground: parse(&part.foreground), border: parse(&part.border), border_width: part.border_width.map(|w| w.clamp(0.0, 6.0)), radius: part.radius.map(|r| r.clamp(0.0, 40.0)) })
        })
        .collect();
    let gradient: Vec<Color> = theme.backdrop.gradient.iter().filter_map(|written| paperloom_theme::parse_color(written)).map(color).collect();
    let layers = theme
        .layer
        .iter()
        .map(|layer| {
            let pictures = if layer.frames.is_empty() { vec![layer.image.clone()] } else { layer.frames.clone() };
            LayerPictures { spec: layer.clone(), frames: pictures.iter().filter_map(|picture| file(picture)).map(Handle::from_path).collect() }
        })
        .filter(|layer| !layer.frames.is_empty())
        .collect();
    Active {
        dark: tokens_of(&dark),
        light: tokens_of(&light),
        dark_extra: (color(dark.overlay), color(dark.shadow)),
        light_extra: (color(light.overlay), color(light.shadow)),
        shape: theme.shape.clamped(),
        layout: theme.layout.clamped(),
        body: font(&theme.fonts.body, DEFAULT_BODY),
        display: font(&theme.fonts.display, DEFAULT_DISPLAY),
        body_family: if theme.fonts.body.trim().is_empty() { DEFAULT_BODY.to_string() } else { theme.fonts.body.trim().to_string() },
        parts,
        gradient: if gradient.len() >= 2 { gradient } else { Vec::new() },
        backdrop: file(&theme.backdrop.image).map(Handle::from_path),
        layers,
        font_files: theme.fonts.files.iter().filter_map(|font| file(font)).collect(),
        listed,
    }
}

static ACTIVE: RwLock<Option<Arc<Active>>> = RwLock::new(None);
/// Whether the window is being drawn dark right now (set by each `view`).
static DARK: AtomicBool = AtomicBool::new(true);

fn default_listed() -> Listed {
    Listed { theme: paperloom_theme::builtin().into_iter().next().unwrap_or_default(), origin: paperloom_theme::Origin::BuiltIn }
}

/// The theme in use.
pub fn active() -> Arc<Active> {
    if let Some(active) = ACTIVE.read().unwrap_or_else(|poisoned| poisoned.into_inner()).as_ref() {
        return active.clone();
    }
    let built = Arc::new(build(default_listed()));
    *ACTIVE.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(built.clone());
    built
}

fn install(listed: Listed) -> Arc<Active> {
    let built = Arc::new(build(listed));
    *ACTIVE.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(built.clone());
    built
}

/// Switches to the theme called `id` (empty = Paperloom's own). A theme
/// that is missing or can't be read leaves the default in place, and says so.
pub fn activate(id: &str) -> Result<Arc<Active>, String> {
    match paperloom_theme::find(&paperloom_common::themes_dir(), id) {
        Some(listed) => Ok(install(listed)),
        None => {
            install(default_listed());
            Err(format!("The theme “{id}” isn't on this computer; Paperloom's own is in use."))
        }
    }
}

/// Draws with `theme` without it being saved anywhere (the editor's preview).
pub fn preview(theme: paperloom_theme::Theme, dir: Option<PathBuf>) -> Arc<Active> {
    install(Listed { theme, origin: dir.map_or(paperloom_theme::Origin::BuiltIn, paperloom_theme::Origin::Installed) })
}

/// Turns the user's setting into a concrete Light/Dark: `System` follows
/// the desktop's reported preference (`iced::system`), and stays dark if
/// the desktop doesn't report one.
pub fn resolve(mode: ThemeMode, system: iced::theme::Mode) -> ThemeMode {
    match mode {
        ThemeMode::System if system == iced::theme::Mode::Light => ThemeMode::Light,
        ThemeMode::System => ThemeMode::Dark,
        explicit => explicit,
    }
}

/// The colours for an already-`resolve`d mode (a raw `System` is dark).
/// Also notes the mode, for `overlay()` and `shadow()`.
pub fn tokens(mode: ThemeMode) -> Tokens {
    let active = active();
    let dark = mode != ThemeMode::Light;
    DARK.store(dark, Ordering::Relaxed);
    if dark {
        active.dark
    } else {
        active.light
    }
}

pub fn from_mode(mode: ThemeMode) -> Theme {
    let name = match mode {
        ThemeMode::Light => "Paperloom Light",
        _ => "Paperloom Dark",
    };
    from_tokens(name, tokens(mode))
}

pub fn from_tokens(name: &'static str, t: Tokens) -> Theme {
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

/// Ordinary text. iced takes its default font once, at start (`main.rs`),
/// so a theme's body font shows after a restart; this is the same font.
pub fn body_font() -> Font {
    active().body
}

/// Page titles and other big text.
pub fn display_font() -> Font {
    active().display
}

/// Corners of buttons, inputs and chips.
pub fn radius_control() -> f32 {
    active().shape.control_radius
}

/// Corners of cards and panels.
pub fn radius_card() -> f32 {
    active().shape.card_radius
}

/// Corners of pop-ups and screens.
pub fn radius_popup() -> f32 {
    active().shape.popup_radius
}

/// Thickness of an outline.
pub fn border_width() -> f32 {
    active().shape.border_width
}

/// The dimming behind a pop-up.
pub fn overlay() -> Color {
    let active = active();
    if DARK.load(Ordering::Relaxed) {
        active.dark_extra.0
    } else {
        active.light_extra.0
    }
}

/// A shadow in the theme's shadow colour: `alpha`, offset and blur are what
/// the default look uses there; the theme's shadow strength scales them.
pub fn shadow(alpha: f32, x: f32, y: f32, blur: f32) -> Shadow {
    let active = active();
    let strength = active.shape.shadow;
    let base = if DARK.load(Ordering::Relaxed) { active.dark_extra.1 } else { active.light_extra.1 };
    Shadow { color: Color { a: (alpha * strength).clamp(0.0, 1.0), ..base }, offset: Vector::new(x, y), blur_radius: blur * strength.min(1.5) }
}

/// A named part's overrides (all `None` when the theme has none).
pub fn part(name: &str) -> PartStyle {
    active().parts.get(name).copied().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_theme_gives_the_colours_the_app_always_had() {
        let active = build(default_listed());
        // #FB923C and #9A3412: the accent in the dark and in the light.
        assert!((active.dark.primary.r - 0.984).abs() < 0.001 && (active.dark.primary.g - 0.573).abs() < 0.001);
        assert!((active.light.primary.r - 0.604).abs() < 0.001 && (active.light.primary.b - 0.071).abs() < 0.001);
        assert!((active.dark.border.a - 0.08).abs() < 0.001);
        assert_eq!((active.shape.control_radius, active.shape.card_radius, active.shape.popup_radius), (4.0, 6.0, 8.0));
        assert!(active.layers.is_empty() && active.backdrop.is_none() && active.gradient.is_empty());
        assert_eq!(active.body_family, DEFAULT_BODY);
    }

    #[test]
    fn a_theme_changes_colours_shape_and_parts() {
        let theme: paperloom_theme::Theme = toml::from_str("id = \"t\"\n[colors.dark]\nprimary = \"#00FF00\"\n[shape]\ncontrol_radius = 99\n[part.nav]\nbackground = \"#FF0000\"\nradius = 12\n[fonts]\ndisplay = \"Some Font\"\n").unwrap();
        let active = build(Listed { theme, origin: paperloom_theme::Origin::BuiltIn });
        assert_eq!((active.dark.primary.r, active.dark.primary.g), (0.0, 1.0));
        assert_eq!(active.shape.control_radius, 24.0, "pulled back into range");
        let nav = active.parts["nav"];
        assert_eq!(nav.background.map(|c| c.r), Some(1.0));
        assert_eq!(nav.radius, Some(12.0));
        assert_eq!(active.display, Font::with_name("Some Font"));
    }
}
