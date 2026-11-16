//! What a `theme.toml` holds. Every field is optional in the file.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::color::{self, Rgba};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    /// Short name used in files and settings: letters, digits, `-`, `_`.
    pub id: String,
    pub name: String,
    pub author: String,
    pub description: String,
    pub version: String,
    pub colors: ColorModes,
    pub fonts: Fonts,
    pub shape: Shape,
    pub layout: Layout,
    pub backdrop: Backdrop,
    /// Pictures placed in the window, behind or in front of everything.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub layer: Vec<Layer>,
    /// Overrides for named parts of the window (see `PARTS`).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub part: BTreeMap<String, Part>,
}

/// The parts a theme can restyle on their own.
pub const PARTS: [(&str, &str); 9] = [
    ("nav", "the navigation (sidebar or bar)"),
    ("top_bar", "the bar with the page title and search"),
    ("button_primary", "the main action button"),
    ("button_secondary", "quiet buttons"),
    ("button_danger", "remove and delete buttons"),
    ("button_ghost", "text-only buttons"),
    ("modal", "small pop-ups"),
    ("screen", "whole-window pop-ups"),
    ("popup", "the search results list"),
];

/// A theme may bring colours for one mode or both. With only one, that
/// one is used whatever the Light/Dark choice is.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorModes {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dark: Option<Colors>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light: Option<Colors>,
}

macro_rules! colors {
    ($($field:ident: $about:literal),* $(,)?) => {
        /// One mode's colours, as written (`#RRGGBB` or `#RRGGBBAA`).
        #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
        #[serde(default)]
        pub struct Colors {
            $(#[serde(skip_serializing_if = "Option::is_none")] pub $field: Option<String>,)*
        }

        /// One mode's colours, resolved: every one has a value.
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct Palette {
            $(pub $field: Rgba,)*
        }

        impl Colors {
            /// The colour names, with what each is for.
            pub const FIELDS: &'static [(&'static str, &'static str)] = &[$((stringify!($field), $about),)*];

            pub fn get(&self, name: &str) -> Option<&String> {
                match name {
                    $(stringify!($field) => self.$field.as_ref(),)*
                    _ => None,
                }
            }

            pub fn set(&mut self, name: &str, value: Option<String>) {
                match name {
                    $(stringify!($field) => self.$field = value,)*
                    _ => {}
                }
            }

            /// These colours over `base` (unreadable values keep the base's).
            pub fn over(&self, base: Palette) -> Palette {
                Palette {
                    $($field: self.$field.as_deref().and_then(color::parse).unwrap_or(base.$field),)*
                }
            }
        }

        impl Palette {
            pub fn get(&self, name: &str) -> Option<Rgba> {
                match name {
                    $(stringify!($field) => Some(self.$field),)*
                    _ => None,
                }
            }
        }
    };
}

colors! {
    background: "behind everything",
    surface: "navigation, cards and pop-ups",
    muted: "quiet buttons and sunken areas",
    border: "outlines and dividing lines",
    foreground: "text",
    muted_foreground: "secondary text",
    primary: "the accent: main buttons, the current page",
    primary_hover: "the accent while pointed at",
    success: "good news (now playing, on)",
    destructive: "remove and delete",
    overlay: "the dimming behind a pop-up",
    shadow: "shadows",
}

impl Palette {
    /// Paperloom's own dark colours (exactly the values the app has always used).
    pub const DARK: Palette = Palette {
        background: [0.110, 0.098, 0.090, 1.0],
        surface: [0.149, 0.133, 0.125, 1.0],
        muted: [0.180, 0.157, 0.141, 1.0],
        border: [1.0, 1.0, 1.0, 0.08],
        foreground: [0.961, 0.937, 0.914, 1.0],
        muted_foreground: [0.659, 0.600, 0.557, 1.0],
        primary: [0.984, 0.573, 0.235, 1.0],
        primary_hover: [0.992, 0.729, 0.455, 1.0],
        success: [0.204, 0.827, 0.600, 1.0],
        destructive: [0.973, 0.443, 0.443, 1.0],
        overlay: [0.0, 0.0, 0.0, 0.5],
        shadow: [0.0, 0.0, 0.0, 0.4],
    };

    /// Paperloom's own light colours.
    pub const LIGHT: Palette = Palette {
        background: [1.0, 0.984, 0.921, 1.0],
        surface: [1.0, 1.0, 1.0, 1.0],
        muted: [0.973, 0.949, 0.941, 1.0],
        border: [0.949, 0.902, 0.886, 1.0],
        foreground: [0.110, 0.078, 0.063, 1.0],
        muted_foreground: [0.471, 0.443, 0.424, 1.0],
        primary: [0.604, 0.204, 0.071, 1.0],
        primary_hover: [0.761, 0.255, 0.047, 1.0],
        success: [0.020, 0.588, 0.412, 1.0],
        destructive: [0.863, 0.149, 0.149, 1.0],
        overlay: [0.0, 0.0, 0.0, 0.5],
        shadow: [0.0, 0.0, 0.0, 0.4],
    };
}

impl Theme {
    /// The colours to draw with in dark (or light) mode. A theme with one
    /// mode only uses it for both.
    pub fn palette(&self, dark: bool) -> Palette {
        let base = if dark { Palette::DARK } else { Palette::LIGHT };
        match (dark, &self.colors.dark, &self.colors.light) {
            (true, Some(colors), _) | (false, _, Some(colors)) => colors.over(base),
            (true, None, Some(only)) => only.over(Palette::LIGHT),
            (false, Some(only), None) => only.over(Palette::DARK),
            (_, None, None) => base,
        }
    }

    /// Does anything about where things sit differ from `other`? (Applying
    /// such a theme asks "Keep this theme?" first.)
    pub fn moves_things(&self, other: &Theme) -> bool {
        self.layout != other.layout || (self.shape.scale - other.shape.scale).abs() > 0.001
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fonts {
    /// Family for all ordinary text. Empty = Paperloom's own.
    pub body: String,
    /// Family for page titles. Empty = Paperloom's own.
    pub display: String,
    /// Font files this theme brings (`fonts/Name.ttf`), relative to its folder.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shape {
    /// Corner roundness of buttons, inputs and chips, in pixels.
    pub control_radius: f32,
    /// Corner roundness of cards and panels.
    pub card_radius: f32,
    /// Corner roundness of pop-ups.
    pub popup_radius: f32,
    /// Thickness of outlines.
    pub border_width: f32,
    /// Shadow strength: 0 = none, 1 = normal, 2 = heavy.
    pub shadow: f32,
    /// Size of everything: 1 = normal, 0.85 = compact, 1.25 = large.
    pub scale: f32,
}

impl Default for Shape {
    fn default() -> Self {
        Self { control_radius: 4.0, card_radius: 6.0, popup_radius: 8.0, border_width: 1.0, shadow: 1.0, scale: 1.0 }
    }
}

impl Shape {
    /// The same shape with every number inside what the app can draw.
    pub fn clamped(self) -> Self {
        Self {
            control_radius: self.control_radius.clamp(0.0, 24.0),
            card_radius: self.card_radius.clamp(0.0, 32.0),
            popup_radius: self.popup_radius.clamp(0.0, 40.0),
            border_width: self.border_width.clamp(0.0, 4.0),
            shadow: self.shadow.clamp(0.0, 3.0),
            scale: self.scale.clamp(0.7, 1.6),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Nav {
    #[default]
    Left,
    Right,
    Top,
    Bottom,
}

impl Nav {
    pub const ALL: [Nav; 4] = [Nav::Left, Nav::Right, Nav::Top, Nav::Bottom];
}

impl std::fmt::Display for Nav {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Nav::Left => "Left",
            Nav::Right => "Right",
            Nav::Top => "Top",
            Nav::Bottom => "Bottom",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NavStyle {
    /// Icons with labels (a tree at the side, a labelled bar on top or bottom).
    #[default]
    Full,
    /// Icons only.
    Icons,
    /// Only a menu button; the navigation opens over the page.
    Hidden,
}

impl NavStyle {
    pub const ALL: [NavStyle; 3] = [NavStyle::Full, NavStyle::Icons, NavStyle::Hidden];
}

impl std::fmt::Display for NavStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            NavStyle::Full => "Icons and labels",
            NavStyle::Icons => "Icons only",
            NavStyle::Hidden => "Menu button only",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    /// Which side the navigation sits on.
    pub nav: Nav,
    pub nav_style: NavStyle,
    /// Width of a side navigation with labels, in pixels.
    pub nav_width: f32,
    /// Show the search box in the top bar.
    pub search: bool,
    /// Show the page's title in the top bar.
    pub title: bool,
    /// Widest the page content may get, in pixels; 0 = as wide as the window.
    pub content_max_width: f32,
}

impl Default for Layout {
    fn default() -> Self {
        Self { nav: Nav::Left, nav_style: NavStyle::Full, nav_width: 200.0, search: true, title: true, content_max_width: 0.0 }
    }
}

impl Layout {
    pub fn clamped(self) -> Self {
        Self { nav_width: self.nav_width.clamp(150.0, 360.0), content_max_width: if self.content_max_width <= 0.0 { 0.0 } else { self.content_max_width.max(480.0) }, ..self }
    }
}

/// What is behind the pages: the background colour by default, or a
/// gradient, or a picture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Backdrop {
    /// Two or more colours for a gradient; empty = none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub gradient: Vec<String>,
    /// Direction of the gradient in degrees (0 = left to right, 90 = top to bottom).
    pub angle: f32,
    /// A PNG in the theme's folder (`images/back.png`); empty = none.
    pub image: String,
    /// 0–1.
    pub opacity: f32,
    /// `cover` (fill the window), `contain` (fit inside) or `stretch`.
    pub fit: String,
}

impl Default for Backdrop {
    fn default() -> Self {
        Self { gradient: Vec::new(), angle: 90.0, image: String::new(), opacity: 1.0, fit: "cover".to_string() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    #[default]
    BottomRight,
}

/// One picture in the window. Several pictures in `frames` play as an
/// animation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layer {
    /// A PNG in the theme's folder.
    pub image: String,
    /// Frames of an animation, played in order and repeated (instead of `image`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<String>,
    /// Frames per second, 1–30.
    pub fps: f32,
    pub anchor: Anchor,
    /// Size in pixels; 0 = the picture's own size.
    pub width: f32,
    pub height: f32,
    /// Distance from the anchored edge(s), in pixels.
    pub margin: f32,
    /// 0–1.
    pub opacity: f32,
    /// In front of the pages (never takes clicks) instead of behind them.
    pub front: bool,
    /// Only on these pages (`library`, `online`, `widgets`, `settings`); empty = everywhere.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pages: Vec<String>,
}

impl Default for Layer {
    fn default() -> Self {
        Self { image: String::new(), frames: Vec::new(), fps: 8.0, anchor: Anchor::BottomRight, width: 0.0, height: 0.0, margin: 0.0, opacity: 1.0, front: false, pages: Vec::new() }
    }
}

/// An override for one named part. Anything left out keeps the theme's
/// general colours and shape.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreground: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_width: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_theme_is_the_default_look() {
        let theme: Theme = toml::from_str("").unwrap();
        assert_eq!(theme.palette(true), Palette::DARK);
        assert_eq!(theme.palette(false), Palette::LIGHT);
        assert_eq!(theme.shape, Shape::default());
        assert_eq!(theme.layout, Layout::default());
    }

    #[test]
    fn one_mode_serves_both() {
        let theme: Theme = toml::from_str("[colors.dark]\nbackground = \"#000000\"\nprimary = \"#00FF00\"").unwrap();
        assert_eq!(theme.palette(true).background, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(theme.palette(false).background, [0.0, 0.0, 0.0, 1.0]);
        // What it left out comes from its own mode's defaults.
        assert_eq!(theme.palette(false).foreground, Palette::DARK.foreground);
    }

    #[test]
    fn round_trips_through_toml() {
        let text = "id = \"x\"\n[layout]\nnav = \"bottom\"\nnav_style = \"icons\"\n[[layer]]\nimage = \"images/a.png\"\nanchor = \"top-left\"\nfront = true\n[part.nav]\nbackground = \"#101010\"\nradius = 12\n";
        let theme: Theme = toml::from_str(text).unwrap();
        assert_eq!(theme.layout.nav, Nav::Bottom);
        assert_eq!(theme.layer[0].anchor, Anchor::TopLeft);
        assert_eq!(theme.part["nav"].radius, Some(12.0));
        let again: Theme = toml::from_str(&toml::to_string_pretty(&theme).unwrap()).unwrap();
        assert_eq!(again, theme);
    }

    #[test]
    fn bad_colours_keep_the_default() {
        let theme: Theme = toml::from_str("[colors.dark]\nprimary = \"orange\"").unwrap();
        assert_eq!(theme.palette(true).primary, Palette::DARK.primary);
    }
}
