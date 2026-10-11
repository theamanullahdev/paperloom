//! The wire protocol between `paperloom-gui` and `paperloom-engine`, over a
//! Unix domain socket. Deliberately tiny — one line in, one line out — not a
//! generic RPC framework, because there are two commands and that's not
//! expected to grow fast.

use std::path::PathBuf;

use crate::paths;

/// Where the engine listens. `$XDG_RUNTIME_DIR` is the right place for a
/// per-user, per-session socket (cleared on logout, not world-readable);
/// falls back to the data dir if it's unset, which happens in some minimal
/// environments.
pub fn socket_path() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir).join("paperloom-engine.sock"),
        _ => paths::library_dir()
            .parent()
            .expect("library_dir always has a parent")
            .join("engine.sock"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Play this file as the wallpaper, replacing whatever's currently playing.
    Play(PathBuf),
    /// Stop playback (the window stays put, just shows nothing).
    Stop,
}

impl Command {
    pub fn encode(&self) -> String {
        match self {
            Command::Play(path) => format!("PLAY {}\n", path.display()),
            Command::Stop => "STOP\n".to_string(),
        }
    }

    pub fn decode(line: &str) -> Option<Command> {
        let line = line.trim_end_matches(['\r', '\n']);
        if line == "STOP" {
            return Some(Command::Stop);
        }
        line.strip_prefix("PLAY ").map(|path| Command::Play(PathBuf::from(path)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_round_trips() {
        let cmd = Command::Play(PathBuf::from("/home/user/Videos/loop.mp4"));
        assert_eq!(Command::decode(&cmd.encode()), Some(cmd));
    }

    #[test]
    fn stop_round_trips() {
        assert_eq!(Command::decode(&Command::Stop.encode()), Some(Command::Stop));
    }

    #[test]
    fn garbage_decodes_to_none() {
        assert_eq!(Command::decode("not a command\n"), None);
    }
}
