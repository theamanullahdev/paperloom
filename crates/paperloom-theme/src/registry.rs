//! Which themes there are: the ones built into the app and the ones in the
//! user's themes folder (downloaded, imported or made in the editor).

use std::path::{Path, PathBuf};

use crate::model::Theme;

/// The theme used when none is chosen, and the fallback for a broken one.
pub const DEFAULT_ID: &str = "paperloom";

const BUILT_IN: [&str; 4] = [
    include_str!("../themes/paperloom.toml"),
    include_str!("../themes/mirror.toml"),
    include_str!("../themes/topline.toml"),
    include_str!("../themes/contrast.toml"),
];

#[derive(Debug, Clone, PartialEq)]
pub enum Origin {
    BuiltIn,
    /// Its folder (holding `theme.toml`, fonts, pictures).
    Installed(PathBuf),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Listed {
    pub theme: Theme,
    pub origin: Origin,
}

impl Listed {
    pub fn dir(&self) -> Option<&Path> {
        match &self.origin {
            Origin::BuiltIn => None,
            Origin::Installed(dir) => Some(dir),
        }
    }

    pub fn built_in(&self) -> bool {
        self.origin == Origin::BuiltIn
    }
}

/// The themes Paperloom ships with, the default first.
pub fn builtin() -> Vec<Theme> {
    BUILT_IN.iter().filter_map(|text| toml::from_str(text).ok()).collect()
}

/// The themes in `themes_dir` (one folder each). A file that can't be read
/// is skipped; a theme without an id is known by its folder's name.
pub fn installed(themes_dir: &Path) -> Vec<Listed> {
    let Ok(entries) = std::fs::read_dir(themes_dir) else { return Vec::new() };
    let mut found: Vec<Listed> = entries
        .flatten()
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|entry| {
            let dir = entry.path();
            let mut theme: Theme = toml::from_str(&std::fs::read_to_string(dir.join("theme.toml")).ok()?).ok()?;
            let folder = entry.file_name().to_string_lossy().into_owned();
            if theme.id.is_empty() {
                theme.id = folder.clone();
            }
            if theme.name.is_empty() {
                theme.name = folder;
            }
            Some(Listed { theme, origin: Origin::Installed(dir) })
        })
        .collect();
    found.sort_by_key(|listed| listed.theme.name.to_lowercase());
    found
}

/// Every theme: built-in first, then the user's. A user's theme can't take
/// a built-in one's id.
pub fn list(themes_dir: &Path) -> Vec<Listed> {
    let mut all: Vec<Listed> = builtin().into_iter().map(|theme| Listed { theme, origin: Origin::BuiltIn }).collect();
    for listed in installed(themes_dir) {
        if !all.iter().any(|known| known.theme.id == listed.theme.id) {
            all.push(listed);
        }
    }
    all
}

/// The theme called `id` (empty = the default).
pub fn find(themes_dir: &Path, id: &str) -> Option<Listed> {
    let id = if id.is_empty() { DEFAULT_ID } else { id };
    list(themes_dir).into_iter().find(|listed| listed.theme.id == id)
}

/// A safe folder name for a theme id.
pub fn folder_name(id: &str) -> String {
    let cleaned: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c.to_ascii_lowercase() } else { '-' }).collect();
    let cleaned = cleaned.trim_matches('-').to_string();
    if cleaned.is_empty() {
        "theme".to_string()
    } else {
        cleaned
    }
}

/// Writes `theme` as `themes_dir/<its id>/theme.toml` (other files in the
/// folder stay). Returns the folder.
pub fn save(themes_dir: &Path, theme: &Theme) -> std::io::Result<PathBuf> {
    if builtin().iter().any(|known| known.id == theme.id) {
        return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, format!("“{}” is a built-in theme's id; pick another id", theme.id)));
    }
    let dir = themes_dir.join(folder_name(&theme.id));
    std::fs::create_dir_all(&dir)?;
    let text = toml::to_string_pretty(theme).map_err(std::io::Error::other)?;
    let staged = dir.join("theme.toml.new");
    std::fs::write(&staged, text)?;
    std::fs::rename(staged, dir.join("theme.toml"))?;
    Ok(dir)
}

/// Deletes an installed theme's folder.
pub fn remove(themes_dir: &Path, id: &str) -> std::io::Result<()> {
    match find(themes_dir, id).map(|listed| listed.origin) {
        Some(Origin::Installed(dir)) if dir.starts_with(themes_dir) => std::fs::remove_dir_all(dir),
        Some(Origin::BuiltIn) => Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "built-in themes can't be removed")),
        _ => Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no such theme")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Palette, Shape};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("paperloom-theme-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn every_built_in_parses_and_checks_clean() {
        assert_eq!(builtin().len(), BUILT_IN.len());
        for text in BUILT_IN {
            let report = crate::check(text, None);
            assert!(report.usable(), "{:?}", report.errors);
            assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        }
    }

    #[test]
    fn the_default_theme_is_the_look_the_app_always_had() {
        let default = builtin().into_iter().next().unwrap();
        assert_eq!(default.id, DEFAULT_ID);
        assert_eq!(default.palette(true), Palette::DARK);
        assert_eq!(default.palette(false), Palette::LIGHT);
        assert_eq!(default.shape, Shape::default());
        assert_eq!(default.layout, crate::Layout::default());
        assert!(default.layer.is_empty() && default.part.is_empty());
    }

    #[test]
    fn save_find_and_remove() {
        let dir = scratch("registry");
        let theme = Theme { id: "My Theme!".into(), name: "Mine".into(), ..Theme::default() };
        let folder = save(&dir, &theme).unwrap();
        assert_eq!(folder.file_name().unwrap(), "my-theme");
        let found = find(&dir, "My Theme!").unwrap();
        assert!(!found.built_in());
        assert_eq!(list(&dir).len(), BUILT_IN.len() + 1);
        assert!(save(&dir, &Theme { id: DEFAULT_ID.into(), ..Theme::default() }).is_err());
        assert!(remove(&dir, DEFAULT_ID).is_err());
        remove(&dir, "My Theme!").unwrap();
        assert!(find(&dir, "My Theme!").is_none());
        assert_eq!(find(&dir, "").unwrap().theme.id, DEFAULT_ID);
        let _ = std::fs::remove_dir_all(dir);
    }
}
