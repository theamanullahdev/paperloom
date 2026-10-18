//! Keeps the desktop's *own* background setting in step with the live
//! wallpaper (Linux only).
//!
//! The video window covers the desktop, but some shell UI doesn't look at
//! windows at all: Cinnamon's workspace switcher animation and Expo
//! overview, GNOME's overview, and the lock screen all draw the background
//! from `gsettings`. Without this they show the old picture. So while a
//! video plays, the system background is a still frame of it; the user's
//! own setting is saved first (`EngineState::saved_background`) and put
//! back when the wallpaper is stopped.
//!
//! Best-effort throughout: no `gsettings`, no `ffmpeg`, an unknown desktop
//! or a Flatpak sandbox (whose `gsettings` writes to the sandbox, not the
//! host) all mean "leave the background alone", never an error.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::thread::JoinHandle;

use paperloom_common::{EngineState, SavedSetting};

/// The background keys of the running desktop: `(schema, key)` pairs
/// whose value is a `file://` URI.
fn background_keys() -> Vec<(&'static str, &'static str)> {
    if std::env::var_os("FLATPAK_ID").is_some() {
        return Vec::new();
    }
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_ascii_lowercase();
    if desktop.contains("cinnamon") {
        vec![("org.cinnamon.desktop.background", "picture-uri")]
    } else if desktop.contains("gnome") || desktop.contains("unity") || desktop.contains("budgie") {
        vec![("org.gnome.desktop.background", "picture-uri"), ("org.gnome.desktop.background", "picture-uri-dark")]
    } else {
        Vec::new()
    }
}

fn gsettings_get(schema: &str, key: &str) -> Option<String> {
    let out = Command::new("gsettings").args(["get", schema, key]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn gsettings_set(schema: &str, key: &str, value: &str) -> bool {
    Command::new("gsettings").args(["set", schema, key, value]).status().is_ok_and(|status| status.success())
}

/// The in-flight `show_frame_of`, so `restore` can wait for it instead of
/// racing it (a quick Play→Stop must not leave the still frame behind).
static PENDING: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

fn wait_for_pending() {
    let pending = PENDING.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
    if let Some(handle) = pending {
        let _ = handle.join();
    }
}

/// `show_frame_of` on a background thread: extracting the frame runs
/// ffmpeg, which mustn't stall the engine's event loop.
pub fn show_frame_of_in_background(video: &str) {
    wait_for_pending();
    let video = video.to_string();
    let handle = std::thread::spawn(move || show_frame_of(&video));
    *PENDING.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(handle);
}

/// Sets the system background to a still of `video`, saving the user's own
/// background the first time (later wallpapers keep that original).
fn show_frame_of(video: &str) {
    let keys = background_keys();
    if keys.is_empty() {
        return;
    }
    let Some(frame) = extract_frame(video) else { return };

    let mut state = EngineState::load();
    if state.saved_background.is_empty() {
        state.saved_background = keys
            .iter()
            .filter_map(|(schema, key)| {
                gsettings_get(schema, key).map(|value| SavedSetting { schema: schema.to_string(), key: key.to_string(), value })
            })
            .collect();
        if let Err(err) = state.save() {
            eprintln!("paperloom-engine: couldn't save the original background ({err}); leaving it alone");
            return;
        }
    }

    let uri = format!("'file://{}'", frame.display());
    for (schema, key) in keys {
        gsettings_set(schema, key, &uri);
    }
}

/// Puts the user's own background back (no-op if we never replaced it).
pub fn restore() {
    wait_for_pending();
    let mut state = EngineState::load();
    if state.saved_background.is_empty() {
        return;
    }
    for setting in &state.saved_background {
        gsettings_set(&setting.schema, &setting.key, &setting.value);
    }
    state.saved_background.clear();
    let _ = state.save();
    // The frames were only ever there for the background setting.
    let _ = std::fs::remove_dir_all(paperloom_common::background_frames_dir());
}

/// One full-size JPEG frame of `video`. Named after the video (and a new
/// file per video), because the shell only reloads the background when the
/// URI changes.
fn extract_frame(video: &str) -> Option<PathBuf> {
    let dir = paperloom_common::background_frames_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let stem = Path::new(video).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "wallpaper".into());
    let safe: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    let frame = dir.join(format!("{safe}.jpg"));

    // 1s in skips a black fade-in; a clip shorter than that gets its first frame.
    for seek in ["1", "0"] {
        let ok = paperloom_common::process::command("ffmpeg")
            .args(["-y", "-loglevel", "error", "-ss", seek, "-i", video, "-frames:v", "1", "-q:v", "2"])
            .arg(&frame)
            .status()
            .is_ok_and(|status| status.success());
        if ok && frame.metadata().is_ok_and(|m| m.len() > 0) {
            return Some(frame);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real ffmpeg against a generated 2-second clip, into a scratch data
    /// dir (never the real XDG one). Skipped where ffmpeg isn't installed.
    #[test]
    fn extracts_a_frame_from_a_video() {
        let dir = std::env::temp_dir().join(format!("paperloom-bg-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("XDG_DATA_HOME", &dir);
        let video = dir.join("clip with spaces.mp4");
        let made = Command::new("ffmpeg")
            .args(["-y", "-loglevel", "error", "-f", "lavfi", "-i", "testsrc=duration=2:size=320x180:rate=10"])
            .arg(&video)
            .status()
            .is_ok_and(|s| s.success());
        if !made {
            eprintln!("ffmpeg not available; skipping");
            return;
        }

        let frame = extract_frame(&video.to_string_lossy()).expect("a frame");
        assert!(frame.starts_with(&dir), "frame written under the scratch data dir: {}", frame.display());
        assert_eq!(frame.file_name().unwrap(), "clip_with_spaces.jpg");
        assert!(std::fs::metadata(&frame).unwrap().len() > 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
