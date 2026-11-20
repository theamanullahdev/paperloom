//! App themes from outside: the online library (the repository's `themes/`
//! folder, read exactly like the widget looks) and theme files a person
//! hands over (`.paperloomtheme`, a folder, or a bare `theme.toml`).
//!
//! Whatever the way in, a theme only lands in the user's themes folder
//! after `paperloom_theme::check` accepts it, and it can only bring `.toml`,
//! fonts, PNGs and licence texts, inside its own folder.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};

use crate::http;
use crate::looks::{self, Index, Pack, MAX_PACK_BYTES};
use crate::share::{base64url_decode, base64url_encode};

/// Where the catalogue is read from, in order: the `paperloom-assets`
/// repository itself and its GitHub Pages copy.
const BASES: &[&str] = &["https://raw.githubusercontent.com/theamanullahdev/paperloom-assets/main/themes/", "https://theamanullahdev.github.io/paperloom-assets/themes/"];
const FORMAT: u32 = 1;
const PREFIX: &str = "paperloom-theme:1:";

/// Reads the catalogue of themes, trying each address until one answers.
pub fn fetch_index() -> Result<(Index, &'static str), String> {
    let mut last = String::from("no address answered");
    for base in BASES {
        match http::get_text(&format!("{base}index.json")) {
            Ok((body, _)) => match serde_json::from_str::<Index>(&body) {
                Ok(index) if index.format <= FORMAT => return Ok((index, base)),
                Ok(_) => return Err("The online library needs a newer Paperloom. Update from Settings → Updates.".into()),
                Err(err) => last = format!("the library's index is malformed: {err}"),
            },
            Err(err) => last = err.to_string(),
        }
    }
    Err(last)
}

/// A theme's thumbnail (`preview.png` in its folder).
pub fn fetch_preview(base: &str, pack: &Pack) -> Result<Vec<u8>, String> {
    http::get_bytes(&format!("{base}{}/preview.png", pack.folder), 1024 * 1024).map_err(|err| err.to_string())
}

/// Downloads a catalogue theme into `themes_dir/<folder>`: every file is
/// checked against the index's checksum, then the theme file itself is checked.
pub fn install(base: &str, pack: &Pack, themes_dir: &Path) -> Result<(), String> {
    looks::install_from(&format!("{base}{}", pack.folder), pack, themes_dir, "theme.toml")?;
    let dir = themes_dir.join(&pack.folder);
    let report = std::fs::read_to_string(dir.join("theme.toml")).map(|text| paperloom_theme::check(&text, Some(&dir))).map_err(|err| err.to_string())?;
    if !report.usable() {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(format!("this theme can't be used: {}", report.errors.join(" ")));
    }
    Ok(())
}

/// What an import did.
#[derive(Debug, Clone, PartialEq)]
pub struct Imported {
    pub id: String,
    pub name: String,
    /// Things that were ignored or look like mistakes (the theme still works).
    pub warnings: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct Bundle {
    v: u32,
    /// Path inside the theme's folder → the file, base64.
    files: BTreeMap<String, String>,
}

/// Every file of a theme folder that a theme may have, by relative path.
fn read_folder(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    fn walk(root: &Path, dir: &Path, depth: u8, out: &mut BTreeMap<String, Vec<u8>>, total: &mut u64) -> Result<(), String> {
        for entry in std::fs::read_dir(dir).map_err(|err| err.to_string())?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth < 3 && !entry.file_name().to_string_lossy().starts_with('.') {
                    walk(root, &path, depth + 1, out, total)?;
                }
                continue;
            }
            let relative = path.strip_prefix(root).map_err(|err| err.to_string())?.to_string_lossy().replace('\\', "/");
            if relative == "preview.png" || !looks::safe_path(&relative) {
                continue;
            }
            let bytes = std::fs::read(&path).map_err(|err| format!("{relative}: {err}"))?;
            *total += bytes.len() as u64;
            if *total > MAX_PACK_BYTES {
                return Err("this theme is larger than the 12 MB limit".into());
            }
            out.insert(relative, bytes);
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    walk(dir, dir, 0, &mut files, &mut 0)?;
    Ok(files)
}

/// A theme folder as one piece of text (what a `.paperloomtheme` file holds).
pub fn pack(dir: &Path) -> Result<String, String> {
    let files = read_folder(dir)?;
    if !files.contains_key("theme.toml") {
        return Err("this folder has no theme.toml".into());
    }
    let bundle = Bundle { v: FORMAT, files: files.into_iter().map(|(path, bytes)| (path, base64url_encode(&bytes))).collect() };
    let json = serde_json::to_vec(&bundle).map_err(|err| err.to_string())?;
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(&json).map_err(|err| err.to_string())?;
    Ok(format!("{PREFIX}{}", base64url_encode(&encoder.finish().map_err(|err| err.to_string())?)))
}

fn unpack(text: &str) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let cleaned: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let body = cleaned.strip_prefix(PREFIX).ok_or_else(|| if cleaned.starts_with("paperloom-theme:") { "This theme file needs a newer Paperloom.".to_string() } else { "This isn't a Paperloom theme file.".to_string() })?;
    let packed = base64url_decode(body).ok_or("The theme file is damaged.")?;
    let mut json = Vec::new();
    DeflateDecoder::new(packed.as_slice()).take(MAX_PACK_BYTES * 2).read_to_end(&mut json).map_err(|_| "The theme file is damaged.")?;
    let bundle: Bundle = serde_json::from_slice(&json).map_err(|_| "The theme file is damaged.")?;
    let mut files = BTreeMap::new();
    let mut total = 0u64;
    for (path, encoded) in bundle.files {
        if !looks::safe_path(&path) {
            return Err(format!("The theme file holds a file it isn't allowed to have: {path}"));
        }
        let bytes = base64url_decode(&encoded).ok_or("The theme file is damaged.")?;
        total += bytes.len() as u64;
        if total > MAX_PACK_BYTES {
            return Err("this theme is larger than the 12 MB limit".into());
        }
        files.insert(path, bytes);
    }
    Ok(files)
}

/// Checks the files and writes them as `themes_dir/<id>/`, replacing an
/// older copy of the same theme.
fn install_files(mut files: BTreeMap<String, Vec<u8>>, fallback_id: &str, themes_dir: &Path) -> Result<Imported, String> {
    let text = String::from_utf8(files.remove("theme.toml").ok_or("There is no theme.toml in it.")?).map_err(|_| "theme.toml isn't text.")?;
    let report = paperloom_theme::check(&text, None);
    let Some(mut theme) = report.theme.clone().filter(|_| report.errors.is_empty()) else {
        return Err(report.errors.join(" "));
    };
    if theme.id.is_empty() {
        theme.id = paperloom_theme::folder_name(fallback_id);
    }
    if theme.name.is_empty() {
        theme.name = theme.id.clone();
    }
    if paperloom_theme::builtin().iter().any(|known| known.id == theme.id) {
        return Err(format!("“{}” is the id of a theme Paperloom ships with. Change the id in theme.toml.", theme.id));
    }
    let folder = paperloom_theme::folder_name(&theme.id);
    let staging = themes_dir.join(format!(".installing-{folder}"));
    let _ = std::fs::remove_dir_all(&staging);
    let written = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(&staging)?;
        // The file as the person wrote it (comments and all), not a re-serialised copy.
        std::fs::write(staging.join("theme.toml"), if report.theme.as_ref().is_some_and(|read| read.id.is_empty()) { format!("id = \"{}\"\n{text}", theme.id) } else { text.clone() })?;
        for (path, bytes) in &files {
            let target = staging.join(path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(target, bytes)?;
        }
        let final_dir = themes_dir.join(&folder);
        let _ = std::fs::remove_dir_all(&final_dir);
        std::fs::rename(&staging, final_dir)
    })();
    if let Err(err) = written {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!("Couldn't save the theme: {err}"));
    }
    // Now that the files are in place, say which named ones are missing.
    let dir = themes_dir.join(&folder);
    let warnings = paperloom_theme::check(&text, Some(&dir)).warnings;
    Ok(Imported { id: theme.id, name: theme.name, warnings })
}

/// Installs a theme from the text of a `.paperloomtheme` file, or from the
/// text of a `theme.toml` (an agent or a person pasting one).
pub fn import_text(text: &str, themes_dir: &Path) -> Result<Imported, String> {
    if text.trim_start().starts_with("paperloom-theme:") {
        return install_files(unpack(text)?, "imported", themes_dir);
    }
    install_files(BTreeMap::from([("theme.toml".to_string(), text.as_bytes().to_vec())]), "imported", themes_dir)
}

/// Installs a theme from a folder, a `theme.toml` or a `.paperloomtheme` file.
pub fn import_path(path: &Path, themes_dir: &Path) -> Result<Imported, String> {
    let stem = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_else(|| "imported".into());
    if path.is_dir() {
        return install_files(read_folder(path)?, &stem, themes_dir);
    }
    let extension = path.extension().map(|extension| extension.to_string_lossy().to_lowercase()).unwrap_or_default();
    if std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0) > MAX_PACK_BYTES * 2 {
        return Err("this file is too large to be a theme".into());
    }
    let text = std::fs::read_to_string(path).map_err(|err| format!("Couldn't read {}: {err}", path.display()))?;
    match extension.as_str() {
        "paperloomtheme" => install_files(unpack(&text)?, &stem, themes_dir),
        // A theme.toml next to its fonts and pictures: take the whole folder.
        "toml" if path.file_name().is_some_and(|name| name == "theme.toml") && path.parent().is_some() => install_files(read_folder(path.parent().unwrap_or(Path::new(".")))?, &stem, themes_dir),
        "toml" => install_files(BTreeMap::from([("theme.toml".to_string(), text.into_bytes())]), &stem, themes_dir),
        _ => Err("Choose a theme folder, a theme.toml or a .paperloomtheme file.".into()),
    }
}

/// Writes theme `id` as a `.paperloomtheme` file at `to`.
pub fn export(theme_dir: &Path, to: &Path) -> Result<PathBuf, String> {
    std::fs::write(to, pack(theme_dir)?).map_err(|err| err.to_string())?;
    Ok(to.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("paperloom-themes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_folder_survives_the_trip_through_a_theme_file() {
        let root = scratch("bundle");
        let source = root.join("source");
        std::fs::create_dir_all(source.join("images")).unwrap();
        std::fs::write(source.join("theme.toml"), "id = \"sea\"\nname = \"Sea\"\n[backdrop]\nimage = \"images/back.png\"\n").unwrap();
        std::fs::write(source.join("images/back.png"), [1u8, 2, 3, 4]).unwrap();
        std::fs::write(source.join("run.sh"), "echo no").unwrap();

        let file = root.join("sea.paperloomtheme");
        export(&source, &file).unwrap();
        let themes = root.join("themes");
        let imported = import_path(&file, &themes).unwrap();
        assert_eq!(imported.id, "sea");
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        assert_eq!(std::fs::read(themes.join("sea/images/back.png")).unwrap(), [1, 2, 3, 4]);
        assert!(!themes.join("sea/run.sh").exists(), "only theme files travel");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pasted_text_and_bad_themes() {
        let root = scratch("text");
        let imported = import_text("name = \"Loose\"\n[colors.dark]\nprimary = \"#00FF00\"\n", &root).unwrap();
        assert_eq!(imported.id, "imported");
        assert!(std::fs::read_to_string(root.join("imported/theme.toml")).unwrap().starts_with("id = \"imported\""));
        assert!(import_text("id = \"x\"\n[colors.dark]\nprimary = \"green\"\n", &root).unwrap_err().contains("isn't a colour"));
        assert!(import_text("id = \"paperloom\"\n", &root).unwrap_err().contains("ships with"));
        assert!(import_text("paperloom-theme:1:!!!!", &root).unwrap_err().contains("damaged"));
        assert!(import_text("paperloom-theme:9:abc", &root).unwrap_err().contains("newer"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_missing_picture_is_a_warning() {
        let root = scratch("missing");
        let imported = import_text("id = \"m\"\n[backdrop]\nimage = \"images/none.png\"\n", &root).unwrap();
        assert!(imported.warnings.iter().any(|warning| warning.contains("left out")));
        let _ = std::fs::remove_dir_all(root);
    }
}
