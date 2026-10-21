use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{paths, ImportMode};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryEntry {
    /// Stable identity (collections refer to wallpapers by it; positions
    /// change on every removal). Assigned on load for older manifests.
    #[serde(default)]
    pub id: u64,
    pub display_name: String,
    pub path: PathBuf,
    pub mode: ImportMode,
    /// The poster shown on a tile at rest: the first of `preview_frames`.
    /// `None` if `ffmpeg` isn't bundled or on `PATH` or extraction failed entirely —
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
    /// Hidden from the library and collections (shown only under Hidden),
    /// without deleting anything.
    #[serde(default)]
    pub hidden: bool,
    /// When it was imported (Unix seconds; 0 for older entries).
    #[serde(default)]
    pub added: u64,
    /// Where this file is in the background "make it cheap to play" step.
    #[serde(default)]
    pub optimize: Optimize,
    /// Size plus a few kilobytes from both ends, to notice the same video
    /// imported twice (even under another name).
    #[serde(default)]
    pub fingerprint: Option<String>,
    /// Starred by the user.
    #[serde(default)]
    pub favorite: bool,
    /// Free-form labels ("rain", "work"), lower-case, no duplicates.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Background optimization state of an imported wallpaper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Optimize {
    /// Older entries, and ones that are never touched: left exactly as they are.
    #[default]
    Untouched,
    /// Waiting for (or in) the background step.
    Pending,
    /// Looked at and already suitable.
    NotNeeded,
    /// Re-encoded for cheaper playback.
    Done,
    /// The re-encode didn't work out; the original stays.
    Failed,
}

/// A named group of wallpapers. Members are wallpaper ids — a link, never a
/// copy — and a wallpaper can be in any number of collections.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Collection {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub members: Vec<u64>,
    #[serde(default)]
    pub hidden: bool,
}

/// The user's wallpaper library: what's been imported and where it actually
/// lives on disk. Kept as an explicit manifest (rather than just scanning
/// `library_dir()`) because `ImportMode::ReferenceInPlace` entries don't
/// live in that folder at all.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Library {
    pub entries: Vec<LibraryEntry>,
    #[serde(default)]
    pub collections: Vec<Collection>,
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
        let mut changed = library.assign_ids();
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

    /// The manifest as saved, without regenerating anything (for read-only
    /// views such as the Settings page's statistics).
    pub fn load_manifest() -> Self {
        let mut library = Self::load_from(&manifest_file());
        library.assign_ids();
        library
    }

    fn load_from(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|contents| toml::from_str(&contents).ok())
            .unwrap_or_default()
    }

    /// Gives every entry without one (older manifests) a unique id; also
    /// drops collection links to wallpapers that no longer exist. Returns
    /// whether anything changed.
    fn assign_ids(&mut self) -> bool {
        let mut changed = false;
        let mut seen = std::collections::HashSet::new();
        for index in 0..self.entries.len() {
            if self.entries[index].id == 0 || !seen.insert(self.entries[index].id) {
                let id = self.next_id();
                self.entries[index].id = id;
                seen.insert(id);
                changed = true;
            }
        }
        for collection in &mut self.collections {
            let before = collection.members.len();
            collection.members.retain(|member| seen.contains(member));
            collection.members.dedup();
            changed |= collection.members.len() != before;
        }
        changed
    }

    fn next_id(&self) -> u64 {
        let entries = self.entries.iter().map(|entry| entry.id);
        let collections = self.collections.iter().map(|collection| collection.id);
        entries.chain(collections).max().unwrap_or(0) + 1
    }

    pub fn entry(&self, id: u64) -> Option<&LibraryEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub fn collection(&self, id: u64) -> Option<&Collection> {
        self.collections.iter().find(|collection| collection.id == id)
    }

    /// Hides or shows a wallpaper.
    pub fn set_hidden(&mut self, id: u64, hidden: bool) -> std::io::Result<()> {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
            entry.hidden = hidden;
        }
        self.save()
    }

    /// Creates an empty collection and returns its id.
    pub fn create_collection(&mut self, name: &str) -> std::io::Result<u64> {
        let id = self.next_id();
        self.collections.push(Collection { id, name: name.trim().to_string(), members: Vec::new(), hidden: false });
        self.save().map(|()| id)
    }

    pub fn rename_collection(&mut self, id: u64, name: &str) -> std::io::Result<()> {
        if let Some(collection) = self.collections.iter_mut().find(|collection| collection.id == id) {
            collection.name = name.trim().to_string();
        }
        self.save()
    }

    pub fn set_collection_hidden(&mut self, id: u64, hidden: bool) -> std::io::Result<()> {
        if let Some(collection) = self.collections.iter_mut().find(|collection| collection.id == id) {
            collection.hidden = hidden;
        }
        self.save()
    }

    /// Deletes the collection only; its wallpapers stay in the library.
    pub fn delete_collection(&mut self, id: u64) -> std::io::Result<()> {
        self.collections.retain(|collection| collection.id != id);
        self.save()
    }

    /// Adds the wallpaper to the collection, or takes it out.
    pub fn set_in_collection(&mut self, collection: u64, entry: u64, member: bool) -> std::io::Result<()> {
        if let Some(collection) = self.collections.iter_mut().find(|c| c.id == collection) {
            let present = collection.members.contains(&entry);
            if member && !present {
                collection.members.push(entry);
            } else if !member && present {
                collection.members.retain(|id| *id != entry);
            }
        }
        self.save()
    }

    /// The collections a wallpaper belongs to.
    pub fn collections_of(&self, entry: u64) -> impl Iterator<Item = &Collection> {
        self.collections.iter().filter(move |collection| collection.members.contains(&entry))
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
        self.import_into(&paths::wallpapers_dir(), source, mode, None, None)?;
        self.save()
    }

    /// Like `import`, for a file that has a better human name than its
    /// file name and a known online origin (see `LibraryEntry::source_id`).
    pub fn import_named(&mut self, source: &Path, mode: ImportMode, display_name: &str, source_id: &str) -> std::io::Result<()> {
        self.import_into(&paths::wallpapers_dir(), source, mode, Some(display_name.to_string()), Some(source_id.to_string()))?;
        self.save()
    }

    pub fn set_favorite(&mut self, id: u64, favorite: bool) -> std::io::Result<()> {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
            entry.favorite = favorite;
        }
        self.save()
    }

    /// Replaces the wallpaper's tags (trimmed, lower-cased, de-duplicated).
    pub fn set_tags(&mut self, id: u64, tags: &str) -> std::io::Result<()> {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
            let mut list: Vec<String> = Vec::new();
            for tag in tags.split(',').map(|tag| tag.trim().trim_start_matches('#').to_lowercase()).filter(|tag| !tag.is_empty()) {
                if !list.contains(&tag) {
                    list.push(tag);
                }
            }
            entry.tags = list;
        }
        self.save()
    }

    /// Wallpapers waiting for the background optimizer.
    pub fn pending_optimizations(&self) -> Vec<(u64, PathBuf)> {
        self.entries.iter().filter(|entry| entry.optimize == Optimize::Pending).map(|entry| (entry.id, entry.path.clone())).collect()
    }

    /// Records the optimizer's verdict: `Ok(Some(path))` the file was
    /// replaced by a cheaper one, `Ok(None)` it was already fine.
    pub fn finish_optimize(&mut self, id: u64, result: Result<Option<PathBuf>, String>) -> std::io::Result<()> {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
            match result {
                Ok(Some(path)) => {
                    entry.path = path;
                    entry.optimize = Optimize::Done;
                }
                Ok(None) => entry.optimize = Optimize::NotNeeded,
                Err(_) => entry.optimize = Optimize::Failed,
            }
            entry.fingerprint = fingerprint(&entry.path);
        }
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

        if let Some(print) = fingerprint(source) {
            if let Some(existing) = self.entries.iter().find(|entry| entry.fingerprint.as_deref() == Some(print.as_str())) {
                return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, format!("already in your library as “{}”", existing.display_name)));
            }
        }

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

        let fingerprint = fingerprint(&path);
        let id = self.next_id();
        let added = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        self.entries.push(LibraryEntry {
            id,
            display_name,
            path,
            mode,
            thumbnail,
            preview_frames,
            preview_version: PREVIEW_VERSION,
            source_id,
            hidden: false,
            added,
            optimize: if mode == ImportMode::CopyAndOptimize { Optimize::Pending } else { Optimize::Untouched },
            fingerprint,
            favorite: false,
            tags: Vec::new(),
        });
        Ok(())
    }

    /// Removes the wallpaper with this id (and its collection links). If
    /// it was copied into the managed library folder, deletes that copy
    /// too; a `ReferenceInPlace` entry's original file is left untouched —
    /// we don't own it.
    pub fn remove(&mut self, id: u64) -> std::io::Result<()> {
        if let Some(index) = self.entries.iter().position(|entry| entry.id == id) {
            self.remove_entry(index)?;
        }
        for collection in &mut self.collections {
            collection.members.retain(|member| *member != id);
        }
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

/// Cheap identity of a file: its size and 64 KB from the start, middle and
/// end. Collisions between different videos are practically impossible and
/// it never reads more than a few hundred kilobytes.
pub fn fingerprint(path: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = fs::File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325 ^ size;
    let mut buffer = vec![0u8; 64 * 1024];
    for offset in [0, size.saturating_sub(size / 2), size.saturating_sub(buffer.len() as u64)] {
        file.seek(SeekFrom::Start(offset.min(size))).ok()?;
        let read = file.read(&mut buffer).ok()?;
        for byte in &buffer[..read] {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    Some(format!("{size:x}-{hash:016x}"))
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
    let ran = crate::process::command("ffmpeg")
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
            id: 7,
            hidden: true,
            added: 1,
            optimize: Optimize::Untouched,
            fingerprint: None,
            favorite: false,
            tags: Vec::new(),
        });
        library.collections.push(Collection { id: 8, name: "Rainy".into(), members: vec![7], hidden: false });
        library.save_to(&manifest_path).unwrap();

        let loaded = Library::load_from(&manifest_path);
        assert_eq!(loaded, library);
    }

    #[test]
    fn ids_and_collections() {
        let mut library = Library::default();
        for name in ["a", "b", "c"] {
            library.entries.push(LibraryEntry {
                id: 0,
                display_name: name.into(),
                path: PathBuf::from(name),
                mode: ImportMode::ReferenceInPlace,
                thumbnail: None,
                preview_frames: Vec::new(),
                preview_version: PREVIEW_VERSION,
                source_id: None,
                hidden: false,
                added: 0,
                optimize: Optimize::Untouched,
                fingerprint: None,
                favorite: false,
                tags: Vec::new(),
            });
        }
        library.collections.push(Collection { id: 50, name: "Old".into(), members: vec![99], hidden: false });
        assert!(library.assign_ids());
        let ids: Vec<u64> = library.entries.iter().map(|e| e.id).collect();
        assert_eq!(ids.len(), 3);
        assert!(ids.iter().all(|id| *id > 50), "new ids never reuse a collection's id");
        assert!(library.collections[0].members.is_empty(), "links to missing wallpapers are dropped");
        assert!(!library.assign_ids(), "stable once assigned");

        library.collections[0].members = vec![ids[0], ids[1]];
        library.collections.push(Collection { id: 60, name: "Two".into(), members: vec![ids[1]], hidden: false });
        assert_eq!(library.collections_of(ids[1]).count(), 2);
        // Removing a wallpaper (without touching disk) unlinks it everywhere.
        let index = library.entries.iter().position(|e| e.id == ids[1]).unwrap();
        library.remove_entry(index).unwrap();
        library.assign_ids();
        assert_eq!(library.collections[0].members, vec![ids[0]]);
        assert!(library.collections[1].members.is_empty());
    }
}
