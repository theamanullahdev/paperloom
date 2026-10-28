//! The looks that are available: the ones that ship with Paperloom, plus the
//! ones installed in the user's skins folder (one folder each, with a
//! `skin.toml` and any fonts and images it uses). The installed ones are read
//! when `refresh` is called, so a look that has just been downloaded shows up
//! without restarting.

use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use crate::element::Skin;

const BUILT_IN: &[&str] = &[
    include_str!("../skins/clock-card.toml"),
    include_str!("../skins/clock-minimal.toml"),
    include_str!("../skins/clock-analog.toml"),
    include_str!("../skins/clock-neon.toml"),
    include_str!("../skins/clock-ring.toml"),
    include_str!("../skins/date-card.toml"),
    include_str!("../skins/date-minimal.toml"),
    include_str!("../skins/date-badge.toml"),
    include_str!("../skins/meter-card.toml"),
    include_str!("../skins/meter-compact.toml"),
    include_str!("../skins/meter-ring.toml"),
    include_str!("../skins/meter-gauge.toml"),
    include_str!("../skins/meter-neon.toml"),
];

fn built_in() -> &'static [Skin] {
    static SKINS: OnceLock<Vec<Skin>> = OnceLock::new();
    SKINS.get_or_init(|| {
        BUILT_IN
            .iter()
            .filter_map(|source| match Skin::parse(source) {
                Ok(skin) => Some(skin),
                Err(err) => {
                    eprintln!("paperloom-skin: a built-in skin didn't parse: {err}");
                    None
                }
            })
            .collect()
    })
}

struct Installed {
    dir: PathBuf,
    skins: Vec<&'static Skin>,
}

static INSTALLED: RwLock<Installed> = RwLock::new(Installed { dir: PathBuf::new(), skins: Vec::new() });

/// Where installed looks live, and reads them. Call once at start-up.
pub fn set_user_dir(dir: &Path) {
    INSTALLED.write().unwrap_or_else(|poisoned| poisoned.into_inner()).dir = dir.to_path_buf();
    refresh();
}

/// Reads the installed looks again (after one was downloaded or removed).
pub fn refresh() {
    let dir = INSTALLED.read().unwrap_or_else(|poisoned| poisoned.into_inner()).dir.clone();
    let mut found: Vec<&'static Skin> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut folders: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()).collect();
        folders.sort();
        for folder in folders {
            let Ok(source) = std::fs::read_to_string(folder.join("skin.toml")) else { continue };
            match Skin::parse(&source) {
                Ok(mut skin) => {
                    skin.dir = Some(folder);
                    // Skins live for the whole run (a few kilobytes each); a refresh replaces the list.
                    found.push(Box::leak(Box::new(skin)));
                }
                Err(err) => eprintln!("paperloom-skin: {} didn't parse: {err}", folder.display()),
            }
        }
    }
    INSTALLED.write().unwrap_or_else(|poisoned| poisoned.into_inner()).skins = found;
}

/// Every look, built-in ones first; an installed look replaces a built-in one with the same id.
pub fn all() -> Vec<&'static Skin> {
    let installed = INSTALLED.read().unwrap_or_else(|poisoned| poisoned.into_inner()).skins.clone();
    let mut skins: Vec<&'static Skin> = built_in().iter().filter(|skin| !installed.iter().any(|other| other.id == skin.id)).collect();
    skins.extend(installed);
    skins
}

pub fn get(id: &str) -> Option<&'static Skin> {
    all().into_iter().find(|skin| skin.id == id)
}

/// Every look that can show `kind`, in order.
pub fn for_kind(kind: &str) -> Vec<&'static Skin> {
    all().into_iter().filter(|skin| skin.kinds.iter().any(|candidate| candidate == kind)).collect()
}

/// The look a new widget of `kind` starts with: its `…/card`.
pub fn default_for(kind: &str) -> Option<&'static Skin> {
    let skins = for_kind(kind);
    skins.iter().copied().find(|skin| skin.id.ends_with("/card")).or_else(|| skins.first().copied())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{preview, render, Text};

    #[test]
    fn every_built_in_skin_parses_and_draws_for_each_of_its_kinds() {
        assert_eq!(built_in().len(), BUILT_IN.len(), "a built-in skin failed to parse");
        let mut text = Text::new();
        for skin in built_in() {
            for kind in &skin.kinds {
                let pixmap = render(skin, &preview(kind), 1.0, &mut text);
                assert!(pixmap.pixels().iter().any(|pixel| pixel.alpha() > 0), "{} draws nothing for {kind}", skin.id);
            }
        }
    }

    #[test]
    fn every_kind_has_a_default_look() {
        for kind in ["clock", "date", "cpu", "memory", "disk", "network", "battery", "uptime"] {
            assert!(default_for(kind).is_some(), "{kind} has no look");
        }
    }

    #[test]
    fn an_installed_look_is_found_and_can_use_its_own_files() {
        let dir = std::env::temp_dir().join(format!("paperloom-skins-test-{}", std::process::id()));
        let folder = dir.join("clock-test");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("skin.toml"), "id = \"clock/test\"\nname = \"Test\"\nkinds = [\"clock\"]\nsize = [100, 40]\n[[element]]\ntype = \"text\"\ntext = \"{time}\"\nx = 0\ny = 0\nsize = 20\n").unwrap();
        set_user_dir(&dir);
        let skin = get("clock/test").expect("installed look is listed");
        assert_eq!(skin.dir.as_deref(), Some(folder.as_path()));
        assert!(for_kind("clock").iter().any(|candidate| candidate.id == "clock/test"));
        set_user_dir(Path::new(""));
        let _ = std::fs::remove_dir_all(dir);
    }
}
