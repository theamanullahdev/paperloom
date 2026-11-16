//! Checks a theme file and says, in sentences, what is wrong (it can't be
//! used) and what is merely odd (it can). The app's import, the editor and
//! the MCP server all show this report.

use std::path::{Component, Path};

use crate::color;
use crate::model::{Colors, Theme, PARTS};

#[derive(Debug, Clone, Default)]
pub struct Report {
    /// The theme, when the file could be read at all.
    pub theme: Option<Theme>,
    /// Reasons it can't be used.
    pub errors: Vec<String>,
    /// Things that were ignored or look like mistakes.
    pub warnings: Vec<String>,
}

impl Report {
    pub fn usable(&self) -> bool {
        self.theme.is_some() && self.errors.is_empty()
    }
}

const TOP: [&str; 12] = ["id", "name", "author", "description", "version", "colors", "fonts", "shape", "layout", "backdrop", "layer", "part"];
const FONTS: [&str; 3] = ["body", "display", "files"];
const SHAPE: [&str; 6] = ["control_radius", "card_radius", "popup_radius", "border_width", "shadow", "scale"];
const LAYOUT: [&str; 6] = ["nav", "nav_style", "nav_width", "search", "title", "content_max_width"];
const BACKDROP: [&str; 5] = ["gradient", "angle", "image", "opacity", "fit"];
const LAYER: [&str; 10] = ["image", "frames", "fps", "anchor", "width", "height", "margin", "opacity", "front", "pages"];
const PART: [&str; 5] = ["background", "foreground", "border", "border_width", "radius"];

/// A file inside the theme's folder: relative, no `..`, no drive or backslash.
fn safe(path: &str) -> bool {
    !path.contains('\\') && !path.contains(':') && Path::new(path).components().all(|part| matches!(part, Component::Normal(_)))
}

fn unknown_keys(table: Option<&toml::Value>, known: &[&str], at: &str, warnings: &mut Vec<String>) {
    let Some(table) = table.and_then(toml::Value::as_table) else { return };
    for key in table.keys().filter(|key| !known.contains(&key.as_str())) {
        warnings.push(format!("{at}{key} isn't something a theme has; it is ignored."));
    }
}

/// Checks `text` (a `theme.toml`). With `dir` (the theme's folder) the
/// fonts and pictures it names are checked too.
pub fn check(text: &str, dir: Option<&Path>) -> Report {
    let mut report = Report::default();
    let value: toml::Value = match toml::from_str(text) {
        Ok(value) => value,
        Err(err) => {
            report.errors.push(format!("The file isn't valid TOML: {}", err.message()));
            return report;
        }
    };
    let theme: Theme = match toml::from_str(text) {
        Ok(theme) => theme,
        Err(err) => {
            report.errors.push(format!("A value has the wrong form: {}", err.message()));
            return report;
        }
    };

    // --- Names nobody reads (typos, mostly).
    let warnings = &mut report.warnings;
    unknown_keys(Some(&value), &TOP, "", warnings);
    let color_names: Vec<&str> = Colors::FIELDS.iter().map(|(name, _)| *name).collect();
    unknown_keys(value.get("colors"), &["dark", "light"], "colors.", warnings);
    for mode in ["dark", "light"] {
        unknown_keys(value.get("colors").and_then(|colors| colors.get(mode)), &color_names, &format!("colors.{mode}."), warnings);
    }
    unknown_keys(value.get("fonts"), &FONTS, "fonts.", warnings);
    unknown_keys(value.get("shape"), &SHAPE, "shape.", warnings);
    unknown_keys(value.get("layout"), &LAYOUT, "layout.", warnings);
    unknown_keys(value.get("backdrop"), &BACKDROP, "backdrop.", warnings);
    for layer in value.get("layer").and_then(toml::Value::as_array).into_iter().flatten() {
        unknown_keys(Some(layer), &LAYER, "layer.", warnings);
    }
    for (name, part) in value.get("part").and_then(toml::Value::as_table).into_iter().flatten() {
        if PARTS.iter().any(|(known, _)| known == name) {
            unknown_keys(Some(part), &PART, &format!("part.{name}."), warnings);
        } else {
            warnings.push(format!("part.{name} isn't a part of the window; it is ignored. Parts: {}.", PARTS.iter().map(|(name, _)| *name).collect::<Vec<_>>().join(", ")));
        }
    }

    // --- The id.
    if theme.id.is_empty() {
        report.warnings.push("There is no id; the folder's name is used.".to_string());
    } else if !theme.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        report.errors.push(format!("id “{}” may only have letters, digits, - and _.", theme.id));
    }

    // --- Colours.
    let mut bad = |at: String, written: &str| report.errors.push(format!("{at}: “{written}” isn't a colour. Write #RRGGBB or #RRGGBBAA."));
    for (mode, colors) in [("dark", &theme.colors.dark), ("light", &theme.colors.light)] {
        let Some(colors) = colors else { continue };
        for (name, _) in Colors::FIELDS {
            if let Some(written) = colors.get(name).filter(|written| color::parse(written).is_none()) {
                bad(format!("colors.{mode}.{name}"), written);
            }
        }
    }
    for (index, written) in theme.backdrop.gradient.iter().enumerate().filter(|(_, written)| color::parse(written).is_none()) {
        bad(format!("backdrop.gradient[{index}]"), written);
    }
    for (name, part) in &theme.part {
        for (field, written) in [("background", &part.background), ("foreground", &part.foreground), ("border", &part.border)] {
            if let Some(written) = written.as_ref().filter(|written| color::parse(written).is_none()) {
                bad(format!("part.{name}.{field}"), written);
            }
        }
    }
    if theme.backdrop.gradient.len() == 1 {
        report.warnings.push("backdrop.gradient needs at least two colours; it is ignored.".to_string());
    }
    for (mode, dark) in [("dark", true), ("light", false)] {
        let palette = theme.palette(dark);
        let ratio = color::contrast(palette.foreground, palette.background);
        if ratio < 3.0 {
            report.warnings.push(format!("In {mode} mode the text is hard to read on the background (contrast {ratio:.1}; 4.5 or more reads well)."));
        }
    }

    // --- Numbers the app pulls back into range.
    if theme.shape.clamped() != theme.shape {
        report.warnings.push("Some [shape] values are out of range and are pulled back (radius 0–24/32/40, border_width 0–4, shadow 0–3, scale 0.7–1.6).".to_string());
    }
    if theme.layout.clamped() != theme.layout {
        report.warnings.push("layout.nav_width is kept between 150 and 360; content_max_width is 0 or at least 480.".to_string());
    }
    if !["cover", "contain", "stretch"].contains(&theme.backdrop.fit.as_str()) {
        report.warnings.push(format!("backdrop.fit “{}” isn't cover, contain or stretch; cover is used.", theme.backdrop.fit));
    }

    // --- Files.
    let mut files: Vec<(String, &str, &[&str])> = Vec::new();
    for font in &theme.fonts.files {
        files.push((font.clone(), "fonts.files", &["ttf", "otf"]));
    }
    if !theme.backdrop.image.is_empty() {
        files.push((theme.backdrop.image.clone(), "backdrop.image", &["png"]));
    }
    for layer in &theme.layer {
        if layer.image.is_empty() && layer.frames.is_empty() {
            report.warnings.push("A [[layer]] has no image and no frames; it shows nothing.".to_string());
        }
        for picture in std::iter::once(&layer.image).chain(&layer.frames).filter(|picture| !picture.is_empty()) {
            files.push((picture.clone(), "layer", &["png"]));
        }
    }
    for (path, at, kinds) in files {
        let extension = Path::new(&path).extension().map(|extension| extension.to_string_lossy().to_lowercase()).unwrap_or_default();
        if !safe(&path) {
            report.errors.push(format!("{at}: “{path}” must be a file inside the theme's own folder."));
        } else if !kinds.contains(&extension.as_str()) {
            report.errors.push(format!("{at}: “{path}” must be a .{} file.", kinds.join(" or .")));
        } else if dir.is_some_and(|dir| !dir.join(&path).is_file()) {
            report.warnings.push(format!("{at}: “{path}” isn't in the theme's folder; it is left out."));
        }
    }

    report.theme = Some(theme);
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_good_theme_passes_clean() {
        let report = check("id = \"calm\"\nname = \"Calm\"\n[colors.dark]\nprimary = \"#4FD1C5\"\n[layout]\nnav = \"right\"\n", None);
        assert!(report.usable(), "{report:?}");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    }

    #[test]
    fn says_what_is_wrong() {
        assert!(check("id = ", None).errors[0].contains("TOML"));
        assert!(check("[layout]\nnav = \"diagonal\"", None).errors[0].contains("wrong form"));
        let report = check("id = \"x\"\n[colors.dark]\nprimary = \"orange\"\n[backdrop]\nimage = \"../../secret.png\"\n", None);
        assert!(report.errors.iter().any(|error| error.contains("colors.dark.primary")));
        assert!(report.errors.iter().any(|error| error.contains("inside the theme's own folder")));
        assert!(!report.usable());
    }

    #[test]
    fn warns_about_typos_and_unreadable_text() {
        let report = check("id = \"x\"\ncolours = 1\n[shape]\nradius = 3\nscale = 9\n[part.sidebar]\nradius = 2\n[colors.dark]\nforeground = \"#222222\"\nbackground = \"#202020\"\n", None);
        assert!(report.usable());
        let all = report.warnings.join("\n");
        for expected in ["colours", "shape.radius", "part.sidebar", "hard to read", "out of range"] {
            assert!(all.contains(expected), "missing {expected} in:\n{all}");
        }
    }

    #[test]
    fn missing_files_are_left_out_not_fatal() {
        let dir = std::env::temp_dir();
        let report = check("id = \"x\"\n[backdrop]\nimage = \"images/definitely-not-here-9f3.png\"\n", Some(&dir));
        assert!(report.usable());
        assert!(report.warnings.iter().any(|warning| warning.contains("left out")));
    }
}
