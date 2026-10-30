//! Small updates: instead of the whole installer, download only the files that
//! changed since the version installed. Each Windows release publishes a
//! manifest (every file of the install with its SHA-256 and size) and each
//! file gzip-compressed under its checksum. The app compares the manifest with
//! the files on disk, downloads just the differences, checks every one against
//! its checksum, and swaps them in (a running program can be renamed on
//! Windows, so the old file moves aside and the new one takes its name).
//!
//! Only for the per-user setup install, where the folder is writable without
//! asking for administrator rights; anything else keeps using the installer.

use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::update::{Asset, InstallKind, Release};

/// The manifest's file name ends with this (`paperloom-0.37.0-windows-x86_64-manifest.json`).
pub const MANIFEST_SUFFIX: &str = "-windows-x86_64-manifest.json";
/// The newest manifest format this version understands.
const FORMAT: u32 = 1;
/// What the "download" leaves behind for the install step.
pub const MARKER_SUFFIX: &str = ".paperloom-update";

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub format: u32,
    pub version: String,
    pub files: Vec<FileEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileEntry {
    /// A plain file name in the install folder.
    pub path: String,
    pub sha256: String,
    pub size: u64,
    /// Size of the compressed download.
    pub packed: u64,
    pub url: String,
}

/// What the install step reads: the new files, already downloaded and checked.
#[derive(Debug, Serialize, Deserialize)]
struct Marker {
    install_dir: PathBuf,
    staging: PathBuf,
    files: Vec<String>,
}

/// The manifest published with `release`, if it has one.
pub fn manifest_asset(release: &Release) -> Option<&Asset> {
    release.assets.iter().find(|asset| asset.name.ends_with(MANIFEST_SUFFIX))
}

/// The folder a small update could go into: this must be the Windows setup
/// install and the folder must be writable without administrator rights.
pub fn install_dir() -> Option<PathBuf> {
    if !cfg!(windows) || InstallKind::detect() != InstallKind::WindowsSetup {
        return None;
    }
    let dir = paperloom_common::process::current_exe()?.parent()?.to_path_buf();
    let probe = dir.join(".paperloom-write-test");
    fs::write(&probe, b"x").ok()?;
    let _ = fs::remove_file(probe);
    Some(dir)
}

/// Whether a small update is possible for this release on this computer.
pub fn offered(release: &Release) -> bool {
    manifest_asset(release).is_some() && install_dir().is_some()
}

fn safe_name(name: &str) -> bool {
    let mut parts = Path::new(name).components();
    matches!((parts.next(), parts.next()), (Some(Component::Normal(_)), None)) && !name.contains(['/', '\\', ':']) && !name.starts_with("unins")
}

/// The files of `manifest` that differ from what is installed in `dir`.
pub fn changed<'a>(manifest: &'a Manifest, dir: &Path) -> Vec<&'a FileEntry> {
    manifest
        .files
        .iter()
        .filter(|file| safe_name(&file.path))
        .filter(|file| crate::update::sha256_file(&dir.join(&file.path)).map(|hash| !hash.eq_ignore_ascii_case(&file.sha256)).unwrap_or(true))
        .collect()
}

/// Reads the manifest (checking it against the release's published digest).
pub fn fetch_manifest(asset: &Asset) -> Result<Manifest, String> {
    let (body, _) = crate::http::get_text(&asset.url).map_err(|err| format!("couldn't read the update list: {err}"))?;
    if let Some(expected) = &asset.sha256 {
        use sha2::{Digest, Sha256};
        let actual: String = Sha256::digest(body.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect();
        if !actual.eq_ignore_ascii_case(expected) {
            return Err("the update list didn't match its checksum".into());
        }
    }
    let manifest: Manifest = serde_json::from_str(&body).map_err(|err| format!("the update list is malformed: {err}"))?;
    if manifest.format > FORMAT {
        return Err("this update needs a newer Paperloom to read it: use the full installer".into());
    }
    Ok(manifest)
}

/// Downloads and checks everything `release` changed. `progress(done, total)` counts
/// compressed bytes across all files. Returns the marker file the install step takes.
pub fn download(release: &Release, cancel: &AtomicBool, mut progress: impl FnMut(u64, u64, &str)) -> Result<(PathBuf, u64, u64), String> {
    let asset = manifest_asset(release).ok_or("this release has no small update")?;
    let dir = install_dir().ok_or("this install can't take a small update")?;
    progress(0, 0, "Checking what changed…");
    let manifest = fetch_manifest(asset)?;
    let todo = changed(&manifest, &dir);
    let total: u64 = todo.iter().map(|file| file.packed).sum();
    let full: u64 = manifest.files.iter().map(|file| file.packed).sum();

    let root = paperloom_common::updates_dir().join(format!("quick-{}", release.version));
    let _ = fs::remove_dir_all(root.join("files"));
    fs::create_dir_all(root.join("files")).map_err(|err| err.to_string())?;

    let mut done = 0u64;
    for file in &todo {
        if !safe_name(&file.path) {
            return Err(format!("the update lists an unsafe file name: {}", file.path));
        }
        let part = root.join(format!("{}.gz", file.sha256));
        progress(done, total, &file.path);
        let mut tries = 0;
        loop {
            tries += 1;
            let base = done;
            match crate::update::attempt(&file.url, &part, cancel, |got, _| progress(base + got, total, &file.path)) {
                Ok(()) => break,
                Err(_) if cancel.load(Ordering::Relaxed) => return Err("Cancelled".into()),
                Err(crate::FetchError::Permanent(message)) => return Err(format!("{}: {message}", file.path)),
                Err(err) if tries >= crate::update::MAX_ATTEMPTS => return Err(format!("{}: {err} (gave up after {tries} attempts)", file.path)),
                Err(_) => std::thread::sleep(std::time::Duration::from_secs(2 * u64::from(tries))),
            }
        }
        done += file.packed;
        // Unpack and check straight away: a bad file is known before anything is replaced.
        unpack(&part, &root.join("files").join(&file.path), &file.sha256, file.size).map_err(|err| format!("{}: {err}", file.path))?;
        let _ = fs::remove_file(&part);
    }

    let marker = Marker { install_dir: dir, staging: root.join("files"), files: todo.iter().map(|file| file.path.clone()).collect() };
    let path = paperloom_common::updates_dir().join(format!("paperloom-{}-windows-x86_64-update{MARKER_SUFFIX}", release.version));
    fs::write(&path, serde_json::to_string(&marker).map_err(|err| err.to_string())?).map_err(|err| err.to_string())?;
    Ok((path, total, full))
}

/// Decompresses `gz` into `target`, checking its SHA-256 and size.
fn unpack(gz: &Path, target: &Path, sha256: &str, size: u64) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let mut reader = flate2::read::GzDecoder::new(fs::File::open(gz).map_err(|err| err.to_string())?);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let mut out = fs::File::create(target).map_err(|err| err.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    let mut written = 0u64;
    loop {
        let read = reader.read(&mut buffer).map_err(|err| format!("couldn't unpack: {err}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        std::io::Write::write_all(&mut out, &buffer[..read]).map_err(|err| err.to_string())?;
        written += read as u64;
    }
    let actual: String = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    if written != size || !actual.eq_ignore_ascii_case(sha256) {
        let _ = fs::remove_file(target);
        return Err("didn't match its checksum".into());
    }
    Ok(())
}

/// Puts the downloaded files in place. Each old file is renamed aside first (a
/// running program can be renamed on Windows, not overwritten), and if
/// anything fails everything is put back.
pub fn apply(marker_path: &Path) -> Result<(), String> {
    apply_in(marker_path, install_dir())
}

fn apply_in(marker_path: &Path, allowed: Option<PathBuf>) -> Result<(), String> {
    let marker: Marker = serde_json::from_str(&fs::read_to_string(marker_path).map_err(|err| err.to_string())?).map_err(|err| err.to_string())?;
    // Only where it was downloaded for: this install, this folder.
    if Some(marker.install_dir.clone()) != allowed {
        return Err("this update was made for a different install".into());
    }
    let mut moved: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    let result = (|| -> Result<(), String> {
        for name in &marker.files {
            if !safe_name(name) {
                return Err(format!("unsafe file name {name}"));
            }
            let (new, target) = (marker.staging.join(name), marker.install_dir.join(name));
            let aside = target.exists().then(|| target.with_file_name(format!("{name}.old")));
            if let Some(aside) = &aside {
                let _ = fs::remove_file(aside);
                fs::rename(&target, aside).map_err(|err| format!("couldn't move {name} aside: {err}"))?;
            }
            moved.push((target.clone(), aside));
            // Same disk or not, a copy always works.
            fs::copy(&new, &target).map_err(|err| format!("couldn't put {name} in place: {err}"))?;
        }
        Ok(())
    })();
    if let Err(err) = result {
        for (target, aside) in moved.into_iter().rev() {
            let _ = fs::remove_file(&target);
            if let Some(aside) = aside {
                let _ = fs::rename(aside, target);
            }
        }
        return Err(err);
    }
    let _ = fs::remove_dir_all(&marker.staging);
    let _ = fs::remove_file(marker_path);
    Ok(())
}

/// Deletes the old files a small update moved aside (what could not go before
/// was still running); called when the app starts.
pub fn cleanup() {
    let Some(dir) = paperloom_common::process::current_exe().and_then(|exe| exe.parent().map(Path::to_path_buf)) else { return };
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "old") {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_names_are_allowed() {
        assert!(safe_name("paperloom-gui.exe"));
        assert!(!safe_name("..\\evil.exe"));
        assert!(!safe_name("sub/evil.exe"));
        assert!(!safe_name("C:evil.exe"));
        assert!(!safe_name("unins000.exe"));
    }

    #[test]
    fn a_failed_swap_puts_everything_back() {
        let root = std::env::temp_dir().join(format!("paperloom-swap-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (install, staging) = (root.join("install"), root.join("staging"));
        fs::create_dir_all(&install).unwrap();
        fs::create_dir_all(&staging).unwrap();
        fs::write(install.join("a.exe"), "old a").unwrap();
        fs::write(install.join("b.dll"), "old b").unwrap();
        fs::write(staging.join("a.exe"), "new a").unwrap();
        fs::write(staging.join("b.dll"), "new b").unwrap();
        let write_marker = |files: &[&str]| {
            let marker = Marker { install_dir: install.clone(), staging: staging.clone(), files: files.iter().map(|name| name.to_string()).collect() };
            let path = root.join("m.paperloom-update");
            fs::write(&path, serde_json::to_string(&marker).unwrap()).unwrap();
            path
        };
        // The second file is missing from the staging folder: nothing may change.
        let broken = write_marker(&["a.exe", "missing.dll"]);
        assert!(apply_in(&broken, Some(install.clone())).is_err());
        assert_eq!(fs::read_to_string(install.join("a.exe")).unwrap(), "old a");
        // Another folder than the one it was made for is refused.
        assert!(apply_in(&broken, Some(root.join("elsewhere"))).is_err());
        // The good one swaps both, leaving the old ones aside.
        let good = write_marker(&["a.exe", "b.dll"]);
        apply_in(&good, Some(install.clone())).unwrap();
        assert_eq!(fs::read_to_string(install.join("a.exe")).unwrap(), "new a");
        assert_eq!(fs::read_to_string(install.join("b.dll")).unwrap(), "new b");
        assert_eq!(fs::read_to_string(install.join("a.exe.old")).unwrap(), "old a");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn unpacking_checks_the_checksum() {
        use flate2::write::GzEncoder;
        use sha2::{Digest, Sha256};
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("paperloom-delta-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let data = b"hello update".repeat(1000);
        let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::best());
        encoder.write_all(&data).unwrap();
        fs::write(dir.join("a.gz"), encoder.finish().unwrap()).unwrap();
        let hash: String = Sha256::digest(&data).iter().map(|byte| format!("{byte:02x}")).collect();
        unpack(&dir.join("a.gz"), &dir.join("out.bin"), &hash, data.len() as u64).unwrap();
        assert_eq!(fs::read(dir.join("out.bin")).unwrap(), data);
        assert!(unpack(&dir.join("a.gz"), &dir.join("bad.bin"), &"0".repeat(64), data.len() as u64).is_err());
        assert!(!dir.join("bad.bin").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
