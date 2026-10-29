//! The online library of widget looks. The catalogue lives in the repository
//! under `widgets/` (an `index.json` and one folder per look); the app reads it
//! straight from there, so a new look is available to everyone as soon as it is
//! merged. A look is a handful of small files (`skin.toml`, fonts, images);
//! every file is checked against the SHA-256 in the index before anything is
//! installed, and nothing outside the look's own folder can be written.

use std::path::{Component, Path};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::http;

/// Where the catalogue is read from, in order: the repository itself (always the
/// latest) and the project's GitHub Pages copy.
const BASES: &[&str] = &["https://raw.githubusercontent.com/theamanullahdev/paperloom/main/widgets/", "https://theamanullahdev.github.io/paperloom/widgets/"];

/// The newest index format this version understands.
const FORMAT: u32 = 1;

/// A look in one install can't be bigger than this (fonts are the heavy part).
const MAX_PACK_BYTES: u64 = 12 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct Index {
    #[serde(default)]
    pub format: u32,
    #[serde(default)]
    pub packs: Vec<Pack>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct Pack {
    /// The look's id, e.g. `clock/orbit`.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    pub kinds: Vec<String>,
    /// Its folder, both in the catalogue and once installed.
    pub folder: String,
    pub files: Vec<PackFile>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct PackFile {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

impl Pack {
    /// What installing it downloads, in bytes.
    pub fn download_size(&self) -> u64 {
        self.files.iter().map(|file| file.size).sum()
    }
}

/// Reads the catalogue, trying each address until one answers.
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

/// A look's preview picture (`preview.png` in its folder).
/// Keeps the catalogue and its previews, so the library still shows when the
/// network is down.
pub fn save_cache(dir: &Path, base: &str, index: &Index, previews: &[(String, Vec<u8>)]) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    if let Ok(json) = serde_json::to_string(&(base, index)) {
        let _ = std::fs::write(dir.join("index.json"), json);
    }
    for (id, bytes) in previews {
        let _ = std::fs::write(dir.join(format!("{}.png", id.replace(['/', '\\'], "_"))), bytes);
    }
}

/// The catalogue as last read: its base address, the index, and the previews kept.
pub type Cached = (String, Index, Vec<(String, Vec<u8>)>);

pub fn load_cache(dir: &Path) -> Option<Cached> {
    let (base, index): (String, Index) = serde_json::from_str(&std::fs::read_to_string(dir.join("index.json")).ok()?).ok()?;
    let previews = index.packs.iter().filter_map(|pack| Some((pack.id.clone(), std::fs::read(dir.join(format!("{}.png", pack.id.replace(['/', '\\'], "_")))).ok()?))).collect();
    Some((base, index, previews))
}

pub fn fetch_preview(base: &str, pack: &Pack) -> Result<Vec<u8>, String> {
    http::get_bytes(&format!("{base}{}/preview.png", pack.folder), 2 * 1024 * 1024).map_err(|err| err.to_string())
}

/// A path a look may contain: relative, inside its folder, and a kind of file a
/// look can use.
fn safe_path(path: &str) -> bool {
    let allowed = Path::new(path).extension().and_then(|extension| extension.to_str()).is_some_and(|extension| {
        ["toml", "ttf", "otf", "png", "txt"].iter().any(|allowed| allowed.eq_ignore_ascii_case(extension))
    });
    allowed && !path.contains('\\') && !path.contains(':') && Path::new(path).components().all(|part| matches!(part, Component::Normal(_)))
}

/// Downloads `pack` into `skins_dir/<folder>`, replacing an older copy. Nothing
/// is installed unless every file arrives and matches its checksum.
pub fn install(base: &str, pack: &Pack, skins_dir: &Path) -> Result<(), String> {
    if pack.folder.is_empty() || !pack.folder.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("this look has an unsafe folder name".into());
    }
    if !pack.files.iter().any(|file| file.path == "skin.toml") {
        return Err("this look has no skin.toml".into());
    }
    if pack.download_size() > MAX_PACK_BYTES {
        return Err("this look is larger than the 12 MB limit".into());
    }
    if let Some(bad) = pack.files.iter().find(|file| !safe_path(&file.path)) {
        return Err(format!("this look lists a file it isn't allowed to have: {}", bad.path));
    }

    let staging = skins_dir.join(format!(".installing-{}", pack.folder));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|err| err.to_string())?;
    let result = (|| -> Result<(), String> {
        for file in &pack.files {
            let bytes = http::get_bytes(&format!("{base}{}/{}", pack.folder, file.path), MAX_PACK_BYTES).map_err(|err| format!("{}: {err}", file.path))?;
            if bytes.len() as u64 != file.size {
                return Err(format!("{} arrived with the wrong size", file.path));
            }
            let digest = format!("{:x}", Sha256::digest(&bytes));
            if !digest.eq_ignore_ascii_case(&file.sha256) {
                return Err(format!("{} didn't match its checksum", file.path));
            }
            let target = staging.join(&file.path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
            }
            std::fs::write(&target, bytes).map_err(|err| err.to_string())?;
        }
        let final_dir = skins_dir.join(&pack.folder);
        let _ = std::fs::remove_dir_all(&final_dir);
        std::fs::rename(&staging, &final_dir).map_err(|err| err.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

/// Removes an installed look.
pub fn remove(folder: &str, skins_dir: &Path) -> Result<(), String> {
    if folder.is_empty() || folder.contains(['/', '\\']) || folder.starts_with('.') {
        return Err("not a look folder".into());
    }
    std::fs::remove_dir_all(skins_dir.join(folder)).map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_files_inside_the_folder_are_allowed() {
        assert!(safe_path("skin.toml"));
        assert!(safe_path("fonts/Orbitron.ttf"));
        assert!(!safe_path("../skin.toml"));
        assert!(!safe_path("/etc/passwd.txt"));
        assert!(!safe_path("fonts\\x.ttf"));
        assert!(!safe_path("C:evil.ttf"));
        assert!(!safe_path("run.sh"));
        assert!(!safe_path("a/../../b.png"));
    }

    #[test]
    fn an_unsafe_pack_is_refused_before_any_download() {
        let pack = Pack {
            id: "clock/x".into(),
            name: "X".into(),
            author: String::new(),
            description: String::new(),
            kinds: vec!["clock".into()],
            folder: "../escape".into(),
            files: vec![PackFile { path: "skin.toml".into(), sha256: String::new(), size: 1 }],
        };
        assert!(install("http://127.0.0.1:1/", &pack, Path::new("/nonexistent")).is_err());
        let missing_skin = Pack { folder: "ok".into(), files: vec![PackFile { path: "font.ttf".into(), sha256: String::new(), size: 1 }], ..pack.clone() };
        assert!(install("http://127.0.0.1:1/", &missing_skin, Path::new("/nonexistent")).is_err());
    }
}

#[cfg(test)]
mod live {
    use super::*;

    /// Reads the real catalogue and installs the first look from it into a scratch folder.
    /// `cargo test -p paperloom-fetch --lib looks::live -- --ignored`
    #[test]
    #[ignore]
    fn the_online_library_installs_a_look() {
        let (index, base) = fetch_index().expect("catalogue");
        assert!(!index.packs.is_empty(), "the catalogue lists looks");
        let pack = index.packs.iter().find(|pack| pack.id == "clock/orbit").unwrap_or(&index.packs[0]);
        assert!(fetch_preview(base, pack).is_ok(), "preview downloads");
        let dir = std::env::temp_dir().join(format!("paperloom-live-looks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        install(base, pack, &dir).expect("installs");
        assert!(dir.join(&pack.folder).join("skin.toml").is_file());
        let _ = std::fs::remove_dir_all(dir);
    }
}
