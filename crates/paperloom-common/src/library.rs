use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{paths, ImportMode};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryEntry {
    pub display_name: String,
    pub path: PathBuf,
    pub mode: ImportMode,
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
    pub fn load() -> Self {
        Self::load_from(&manifest_file())
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
        self.import_into(&paths::library_dir(), source, mode)?;
        self.save()
    }

    /// Core of `import`, parameterized on the library directory so it can
    /// be exercised in tests without touching the real one. Does not save.
    fn import_into(&mut self, dir: &Path, source: &Path, mode: ImportMode) -> std::io::Result<()> {
        let display_name = source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "wallpaper".to_string());

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

        self.entries.push(LibraryEntry { display_name, path, mode });
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

        Ok(())
    }
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
            .import_into(library_dir.path(), &source_file, ImportMode::CopyNoReencode)
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
            .import_into(library_dir.path(), &source_file, ImportMode::ReferenceInPlace)
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
        library.import_into(library_dir.path(), &copied_source, ImportMode::CopyNoReencode).unwrap();
        library.import_into(library_dir.path(), &referenced_source, ImportMode::ReferenceInPlace).unwrap();

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
        });
        library.save_to(&manifest_path).unwrap();

        let loaded = Library::load_from(&manifest_path);
        assert_eq!(loaded, library);
    }
}
