//! `paperloom-gui <verb>`: drive the running wallpaper engine from a terminal,
//! a script or a launcher — no window opens.
//!
//!   next | pause | resume | mute | unmute | stop | ping | status | set <name or path>

use paperloom_common::ipc::{self, Command};
use paperloom_common::{EngineState, Library};

const VERBS: [&str; 9] = ["next", "pause", "resume", "mute", "unmute", "stop", "ping", "status", "set"];

/// `Some(exit code)` if `args` was a command-line verb (already handled).
pub fn run(args: &[String]) -> Option<i32> {
    let verb = args.get(1)?.as_str();
    if verb == "help" || verb == "--help" || verb == "-h" {
        println!("Usage: paperloom-gui [next|pause|resume|mute|unmute|stop|ping|status|set <name or path>]\nWith no command, Paperloom opens its window.");
        return Some(0);
    }
    if !VERBS.contains(&verb) {
        return None;
    }
    let send = |command: Command, done: &str| match ipc::send(&command) {
        Ok(()) => {
            println!("{done}");
            0
        }
        Err(_) => {
            eprintln!("No wallpaper is running (the engine is not started).");
            1
        }
    };
    Some(match verb {
        "next" => send(Command::Next, "Changing wallpaper"),
        "pause" => send(Command::Pause, "Paused"),
        "resume" => send(Command::Resume, "Resumed"),
        "mute" => send(Command::Mute(true), "Muted"),
        "unmute" => send(Command::Mute(false), "Unmuted"),
        "stop" => send(Command::Stop, "Stopped"),
        "ping" => send(Command::Ping, "The engine is running"),
        "status" => {
            let alive = ipc::send(&Command::Ping).is_ok();
            let state = EngineState::load();
            println!("engine: {}", if alive { "running" } else { "not running" });
            println!("wallpaper: {}", state.current_wallpaper.map_or("none".into(), |path| path.display().to_string()));
            println!("paused: {}", state.user_paused || state.auto_paused);
            if alive && !state.engine.codec.is_empty() {
                println!("video: {} {}x{} ({})", state.engine.codec, state.engine.width, state.engine.height, if state.engine.hwdec.is_empty() || state.engine.hwdec == "no" { "software decoding" } else { "hardware decoding" });
            }
            0
        }
        "set" => {
            let wanted = args[2..].join(" ");
            if wanted.is_empty() {
                eprintln!("Usage: paperloom-gui set <part of a name, or a file path>");
                return Some(2);
            }
            let path = std::path::Path::new(&wanted);
            let target = if path.is_file() {
                Some(path.to_path_buf())
            } else {
                let library = Library::load_manifest();
                let needle = wanted.to_lowercase();
                library.entries.iter().find(|entry| entry.display_name.to_lowercase().contains(&needle) && entry.path.is_file()).map(|entry| entry.path.clone())
            };
            match target {
                Some(file) => match crate::engine_client::play(&file) {
                    Ok(()) => {
                        println!("Now playing {}", file.display());
                        0
                    }
                    Err(err) => {
                        eprintln!("Could not start playback: {err}");
                        1
                    }
                },
                None => {
                    eprintln!("No wallpaper in your library matches “{wanted}”.");
                    1
                }
            }
        }
        _ => unreachable!(),
    })
}
