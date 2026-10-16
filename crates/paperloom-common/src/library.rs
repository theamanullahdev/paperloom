use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::{paths, ImportMode};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryEntry {
    pub display_name: String,
    pub path: PathBuf,
    pub mode: ImportMode,
    /// The poster shown on a tile at rest: the first of `preview_frames`.
    /// `None` if `ffmpeg` isn't on `PATH` or extraction failed entirely —
    /// the gallery falls back to a plain icon tile rather than treating
    /// that as fatal.
    #[serde(default)]
    pub thumbnail: Option<PathBuf>,
    /// A short run of *consecutive* frames (`PREVIEW_SECONDS` at
    /// `PREVIEW_FPS`), extracted at import time via `ffmpeg` (already a
    /// hard dependency through `libmpv`/`mpv`). The gallery plays them
    /// ping-pong (forward then back, so it loops without a jump cut) on
    /// the hovered tile only — real motion for the cost of a few small
    /// JPEG decodes, instead of one mpv decoder per tile, which would
    /// break the near-zero-idle requirement.
    #[serde(default)]
    pub preview_frames: Vec<PathBuf>,
    /// Which extraction scheme produced `preview_frames`; entries from an
    /// older scheme are regenerated once on load (see `PREVIEW_VERSION`).
    #[serde(default)]
    pub preview_version: u32,
    /// Set for wallpapers that came from an online source (the download
    /// queue's job id, e.g. `motionbgs.com:10118`), so the online browser
    /// can mark them "In library".
    #[serde(default)]
    pub source_id: Option<String>,
}

/// The user's wallpaper library: what's been imported and where it actually
/// lives on disk. Kept as an explicit manifest (rather than just scanning
/// `library_dir()`) because `ImportMode::ReferenceInPlace` entries don't
/// live in that folder at all.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Library {
    pub entries: Vec<LibraryEntry>,
}

fn manifest_file() -> PathBuf {
    paths::library_dir().join("library.toml")
}

impl Library {
    /// Loads the manifest and (re)generates preview frames for any entry
    /// made under an older extraction scheme. Saves once if anything
    /// changed, so this only pays the `ffmpeg` cost once per entry.
    pub fn load() -> Self {
        let mut library = Self::load_from(&manifest_file());
        let mut changed = false;
        for entry in &mut library.entries {
            if entry.preview_version < PREVIEW_VERSION {
                for old in &entry.preview_frames {
                    let _ = fs::remove_file(old);
                }
                entry.preview_frames = generate_preview_frames(&entry.path);
                entry.thumbnail = entry.preview_frames.first().cloned();
                entry.preview_version = PREVIEW_VERSION;
                changed = true;
            }
        }
        if changed {
            let _ = library.save();
        }
        library
    }

    fn load_from(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|contents| toml::from_str(&contents).ok())
            .unwrap_or_default()
    }

    fn save(&self) -> std::io::Result<()> {
        self.save_to(&manifest_file())
    }

    fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let contents = toml::to_string_pretty(self).expect("Library always serializes");
        fs::write(path, contents)
    }

    /// Imports `source` per `mode` and persists the updated manifest.
    pub fn import(&mut self, source: &Path, mode: ImportMode) -> std::io::Result<()> {
        self.import_into(&paths::library_dir(), source, mode, None, None)?;
        self.save()
    }

    /// Like `import`, for a file that has a better human name than its
    /// file name and a known online origin (see `LibraryEntry::source_id`).
    pub fn import_named(&mut self, source: &Path, mode: ImportMode, display_name: &str, source_id: &str) -> std::io::Result<()> {
        self.import_into(&paths::library_dir(), source, mode, Some(display_name.to_string()), Some(source_id.to_string()))?;
        self.save()
    }

    pub fn contains_source(&self, source_id: &str) -> bool {
        self.entries.iter().any(|entry| entry.source_id.as_deref() == Some(source_id))
    }

    /// Core of `import`, parameterized on the library directory so it can
    /// be exercised in tests without touching the real one. Does not save.
    fn import_into(
        &mut self,
        dir: &Path,
        source: &Path,
        mode: ImportMode,
        display_name: Option<String>,
        source_id: Option<String>,
    ) -> std::io::Result<()> {
        let display_name = display_name.unwrap_or_else(|| {
            source.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| "wallpaper".to_string())
        });

        let path = match mode {
            ImportMode::ReferenceInPlace => source.to_path_buf(),
            // Re-encoding needs the engine's decode pipeline, which doesn't
            // exist yet — both copy modes just copy for now.
            ImportMode::CopyAndOptimize | ImportMode::CopyNoReencode => {
                fs::create_dir_all(dir)?;
                let dest = unique_destination(dir, source);
                fs::copy(source, &dest)?;
                dest
            }
        };

        let preview_frames = generate_preview_frames(&path);
        let thumbnail = preview_frames.first().cloned();

        self.entries.push(LibraryEntry { display_name, path, mode, thumbnail, preview_frames, preview_version: PREVIEW_VERSION, source_id });
        Ok(())
    }

    /// Removes the entry at `index`. If it was copied into the managed
    /// library folder, deletes that copy too; a `ReferenceInPlace` entry's
    /// original file is left untouched — we don't own it.
    pub fn remove(&mut self, index: usize) -> std::io::Result<()> {
        self.remove_entry(index)?;
        self.save()
    }

    fn remove_entry(&mut self, index: usize) -> std::io::Result<()> {
        if index >= self.entries.len() {
            return Ok(());
        }

        let entry = self.entries.remove(index);
        if entry.mode != ImportMode::ReferenceInPlace {
            if let Err(err) = fs::remove_file(&entry.path) {
                if err.kind() != std::io::ErrorKind::NotFound {
                    return Err(err);
                }
            }
        }
        for frame in &entry.preview_frames {
            let _ = fs::remove_file(frame);
        }

        Ok(())
    }
}

/// Bump when the extraction scheme changes so existing entries regenerate.
const PREVIEW_VERSION: u32 = 1;
const PREVIEW_START_SECONDS: u32 = 1;
const PREVIEW_SECONDS: u32 = 3;
const PREVIEW_FPS: u32 = 10;
const PREVIEW_WIDTH: u32 = 320;

/// Extracts `PREVIEW_SECONDS` of consecutive frames starting just past the
/// likely-black opening, as small JPEGs, in a single `ffmpeg` run.
/// Best-effort: a missing `ffmpeg` or an unreadable clip means no frames
/// (a clip shorter than the window just yields fewer), never an import
/// failure.
fn generate_preview_frames(video: &Path) -> Vec<PathBuf> {
    let dir = paths::thumbnails_dir();
    if fs::create_dir_all(&dir).is_err() {
        return Vec::new();
    }

    let stem = video.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "wallpaper".to_string());
    // A stem no existing preview already uses, so two wallpapers with the
    // same file name never overwrite each other's frames.
    let unique_stem = (0u32..)
        .map(|n| if n == 0 { stem.clone() } else { format!("{stem}-{n}") })
        .find(|candidate| !dir.join(format!("{candidate}-m01.jpg")).exists())
        .expect("an unused stem exists");

    let pattern = dir.join(format!("{unique_stem}-m%02d.jpg"));
    let ran = Command::new("ffmpeg")
        .args(["-y", "-ss", &PREVIEW_START_SECONDS.to_string(), "-t", &PREVIEW_SECONDS.to_string(), "-i"])
        .arg(video)
        .args(["-vf", &format!("fps={PREVIEW_FPS},scale={PREVIEW_WIDTH}:-1"), "-q:v", "5"])
        .arg(&pattern)
        .output()
        .is_ok_and(|out| out.status.success());
    if !ran {
        return Vec::new();
    }

    (1..=PREVIEW_SECONDS * PREVIEW_FPS)
        .map(|n| dir.join(format!("{unique_stem}-m{n:02}.jpg")))
        .take_while(|frame| frame.metadata().is_ok_and(|m| m.len() > 0))
        .collect()
}

/// Picks a free filename in `dir` for `source`, appending `-1`, `-2`, ... on
/// collision rather than silently overwriting an existing import.
fn unique_destination(dir: &Path, source: &Path) -> PathBuf {
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "wallpaper".to_string());
    let ext = source.extension().map(|e| e.to_string_lossy().into_owned());

    let mut attempt = 0u32;
    loop {
        let name = match (&ext, attempt) {
            (Some(ext), 0) => format!("{stem}.{ext}"),
            (Some(ext), n) => format!("{stem}-{n}.{ext}"),
            (None, 0) => stem.clone(),
            (None, n) => format!("{stem}-{n}"),
        };
        let candidate = dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
        attempt += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory under the OS temp dir, cleaned up on drop, so
    /// tests never touch the real `paths::library_dir()`.
    struct ScratchDir(PathBuf);

    impl ScratchDir {
        fn new(label: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "paperloom-test-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn touch(path: &Path, contents: &str) {
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn copy_import_copies_into_library_dir() {
        let source_dir = ScratchDir::new("source");
        let library_dir = ScratchDir::new("library");
        let source_file = source_dir.path().join("loop.mp4");
        touch(&source_file, "fake mp4 bytes");

        let mut library = Library::default();
        library
            .import_into(library_dir.path(), &source_file, ImportMode::CopyNoReencode, None, None)
            .unwrap();

        assert_eq!(library.entries.len(), 1);
        let entry = &library.entries[0];
        assert_eq!(entry.display_name, "loop.mp4");
        assert!(entry.path.starts_with(library_dir.path()));
        assert!(entry.path.exists());
        assert_eq!(fs::read_to_string(&entry.path).unwrap(), "fake mp4 bytes");
    }

    #[test]
    fn reference_in_place_does_not_copy() {
        let source_dir = ScratchDir::new("source");
        let library_dir = ScratchDir::new("library");
        let source_file = source_dir.path().join("loop.mp4");
        touch(&source_file, "fake mp4 bytes");

        let mut library = Library::default();
        library
            .import_into(library_dir.path(), &source_file, ImportMode::ReferenceInPlace, None, None)
            .unwrap();

        assert_eq!(library.entries[0].path, source_file);
        assert!(fs::read_dir(library_dir.path()).unwrap().next().is_none());
    }

    #[test]
    fn remove_deletes_copied_file_but_not_referenced_original() {
        let source_dir = ScratchDir::new("source");
        let library_dir = ScratchDir::new("library");
        let copied_source = source_dir.path().join("copied.mp4");
        let referenced_source = source_dir.path().join("referenced.mp4");
        touch(&copied_source, "a");
        touch(&referenced_source, "b");

        let mut library = Library::default();
        library.import_into(library_dir.path(), &copied_source, ImportMode::CopyNoReencode, None, None).unwrap();
        library.import_into(library_dir.path(), &referenced_source, ImportMode::ReferenceInPlace, None, None).unwrap();

        let copied_path = library.entries[0].path.clone();
        assert!(copied_path.exists());

        library.remove_entry(0).unwrap();
        assert!(!copied_path.exists(), "copied file should be deleted");
        assert_eq!(library.entries.len(), 1);

        library.remove_entry(0).unwrap();
        assert!(referenced_source.exists(), "referenced original must survive removal");
        assert!(library.entries.is_empty());
    }

    #[test]
    fn unique_destination_avoids_collisions() {
        let dir = ScratchDir::new("collide");
        let source = Path::new("loop.mp4");

        let first = unique_destination(dir.path(), source);
        touch(&first, "x");
        let second = unique_destination(dir.path(), source);

        assert_ne!(first, second);
        assert_eq!(second, dir.path().join("loop-1.mp4"));
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = ScratchDir::new("manifest");
        let manifest_path = dir.path().join("library.toml");

        let mut library = Library::default();
        library.entries.push(LibraryEntry {
            display_name: "loop.mp4".into(),
            path: PathBuf::from("/tmp/whatever/loop.mp4"),
            mode: ImportMode::ReferenceInPlace,
            thumbnail: None,
            preview_frames: Vec::new(),
            preview_version: PREVIEW_VERSION,
            source_id: Some("motionbgs.com:1".into()),
        });
        library.save_to(&manifest_path).unwrap();

        let loaded = Library::load_from(&manifest_path);
        assert_eq!(loaded, library);
    }
}
