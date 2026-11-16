//! What is on the desktop right now: status, set, next, pause, mute, stop.

use std::path::Path;
use std::time::{Duration, Instant};

use paperloom_common::ipc::{self, Command};
use paperloom_common::{EngineState, Library};
use serde_json::{json, Value};

use super::{Kind, Tool};
use crate::args;

pub fn tools() -> Vec<Tool> {
    vec![
        Tool { name: "status", about: "What is playing, whether it is paused or muted, and how big the library is.", kind: Kind::Read, fields: &[], run: status },
        Tool {
            name: "set_wallpaper",
            about: "Put a wallpaper from the library (or any video/image file) on the desktop. Starts the wallpaper engine if needed.",
            kind: Kind::Change,
            fields: &[("wallpaper", "string", "A library wallpaper's id or (part of) its name, or the full path of a file.", true)],
            run: set,
        },
        Tool { name: "next_wallpaper", about: "Change to the next wallpaper of the active rotation.", kind: Kind::Change, fields: &[], run: |_| send(Command::Next, "Changing wallpaper.") },
        Tool { name: "pause", about: "Hold the wallpaper on its current frame until resume.", kind: Kind::Change, fields: &[], run: |_| send(Command::Pause, "Paused.") },
        Tool { name: "resume", about: "Play the wallpaper again after pause.", kind: Kind::Change, fields: &[], run: |_| send(Command::Resume, "Playing.") },
        Tool { name: "mute", about: "Turn the wallpaper's sound off.", kind: Kind::Change, fields: &[], run: |_| mute(true) },
        Tool { name: "unmute", about: "Turn the wallpaper's sound on.", kind: Kind::Change, fields: &[], run: |_| mute(false) },
        Tool { name: "stop_wallpaper", about: "Take the live wallpaper off the desktop (the desktop's own background comes back).", kind: Kind::Change, fields: &[], run: |_| send(Command::Stop, "Stopped.") },
    ]
}

fn status(_args: &Value) -> Result<Value, String> {
    let running = ipc::send(&Command::Ping).is_ok();
    let state = EngineState::load();
    let library = Library::load_manifest();
    let settings = paperloom_common::load();
    let playing = state.current_wallpaper.as_ref().filter(|_| running).map(|path| {
        let entry = library.entries.iter().find(|entry| &entry.path == path);
        json!({ "path": path, "name": entry.map(|entry| entry.display_name.clone()), "id": entry.map(|entry| entry.id) })
    });
    let video = (running && !state.engine.codec.is_empty()).then(|| {
        json!({
            "codec": state.engine.codec,
            "width": state.engine.width,
            "height": state.engine.height,
            "fps": state.engine.fps,
            "hardware_decoding": !(state.engine.hwdec.is_empty() || state.engine.hwdec == "no"),
        })
    });
    Ok(json!({
        "paperloom_version": env!("CARGO_PKG_VERSION"),
        "engine_running": running,
        "wallpaper": playing,
        "paused": running && (state.user_paused || state.auto_paused),
        "pause_reason": if state.user_paused { "paused by the user".to_string() } else if state.auto_paused { state.pause_reason.clone() } else { String::new() },
        "muted": settings.muted,
        "video": video,
        "library": { "wallpapers": library.entries.len(), "collections": library.collections.len() },
        "widgets_on": paperloom_common::widgets::Widgets::load().enabled,
    }))
}

fn set(args: &Value) -> Result<Value, String> {
    let wanted = args::need_text(args, "wallpaper")?;
    let library = Library::load_manifest();
    let (path, name) = match super::library::find(&library, &wanted) {
        Ok(entry) => (entry.path.clone(), entry.display_name.clone()),
        Err(not_found) => {
            let path = Path::new(&wanted);
            if !path.is_file() {
                return Err(not_found);
            }
            (path.to_path_buf(), wanted.clone())
        }
    };
    if !path.is_file() {
        return Err(format!("“{name}” is in the library but its file is gone ({}).", path.display()));
    }
    play(&path)?;
    Ok(json!({ "playing": name, "path": path }))
}

/// How long a cold start may take (Windows scans the engine's big DLL first).
const STARTUP_WAIT: Duration = if cfg!(windows) { Duration::from_secs(45) } else { Duration::from_secs(8) };

/// Plays `path`, starting the engine (next to this program) when none runs.
pub fn play(path: &Path) -> Result<(), String> {
    let command = Command::Play(path.to_path_buf());
    if ipc::send(&command).is_ok() {
        return Ok(());
    }
    let engine = paperloom_common::process::sibling_binary("paperloom-engine").ok_or("The wallpaper engine (paperloom-engine) isn't installed next to paperloom-mcp.")?;
    let mut start = std::process::Command::new(engine);
    paperloom_common::process::hide_console(&mut start);
    let mut child = start
        .arg("--on-demand")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|err| format!("Couldn't start the wallpaper engine: {err}"))?;
    let started = Instant::now();
    while started.elapsed() < STARTUP_WAIT {
        std::thread::sleep(Duration::from_millis(100));
        if ipc::send(&command).is_ok() {
            return Ok(());
        }
        if let Ok(Some(_)) = child.try_wait() {
            let why = std::fs::read_to_string(paperloom_common::engine_log()).unwrap_or_default();
            return Err(format!("The wallpaper engine stopped at startup. {}", why.trim()));
        }
    }
    Err("The wallpaper engine didn't start in time.".to_string())
}

fn send(command: Command, done: &str) -> Result<Value, String> {
    ipc::send(&command).map(|()| json!({ "ok": true, "message": done })).map_err(|_| "No wallpaper is running. Use set_wallpaper first.".to_string())
}

fn mute(muted: bool) -> Result<Value, String> {
    // The saved setting is what the engine reads on its next start.
    let mut settings = paperloom_common::load();
    settings.muted = muted;
    paperloom_common::save(&settings).map_err(|err| format!("Couldn't save the setting: {err}"))?;
    let _ = ipc::send(&Command::Mute(muted));
    Ok(json!({ "ok": true, "muted": muted }))
}
