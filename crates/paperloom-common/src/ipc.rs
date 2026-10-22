//! The wire protocol between `paperloom-gui` and `paperloom-engine`.
//! Deliberately tiny — one line in, nothing back — not a generic RPC
//! framework, because there are a handful of commands and that's not
//! expected to grow fast.
//!
//! Transport: a Unix domain socket on Linux/macOS, a named pipe on
//! Windows (std has no `AF_UNIX` there). Both sides only ever go through
//! `Listener::accept_command` / `send`, so neither the engine backends nor
//! the GUI need to know which one is in use.

use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Play this file as the wallpaper, replacing whatever's currently playing.
    Play(PathBuf),
    /// Stop playback (the window stays put, just shows nothing).
    Stop,
    /// Exit the engine. Used before an update is installed (Windows can't
    /// replace a running executable) and is otherwise harmless.
    Quit,
    /// Hold the wallpaper on its current frame until `Resume` (the tray's
    /// Pause), regardless of the automatic pause triggers.
    Pause,
    Resume,
    /// Mute or unmute the playing wallpaper right now (the tray widget's
    /// switch); the saved setting is changed by whoever sends this.
    Mute(bool),
    /// Does nothing; answers "is the engine alive" (Developers page, CLI).
    Ping,
    /// Change to the next wallpaper of the active rotation now.
    Next,
}

impl Command {
    pub fn encode(&self) -> String {
        match self {
            Command::Play(path) => format!("PLAY {}\n", path.display()),
            Command::Stop => "STOP\n".to_string(),
            Command::Quit => "QUIT\n".to_string(),
            Command::Pause => "PAUSE\n".to_string(),
            Command::Resume => "RESUME\n".to_string(),
            Command::Mute(true) => "MUTE\n".to_string(),
            Command::Mute(false) => "UNMUTE\n".to_string(),
            Command::Ping => "PING\n".to_string(),
            Command::Next => "NEXT\n".to_string(),
        }
    }

    pub fn decode(line: &str) -> Option<Command> {
        let line = line.trim_end_matches(['\r', '\n']);
        match line {
            "STOP" => Some(Command::Stop),
            "QUIT" => Some(Command::Quit),
            "PAUSE" => Some(Command::Pause),
            "RESUME" => Some(Command::Resume),
            "MUTE" => Some(Command::Mute(true)),
            "UNMUTE" => Some(Command::Mute(false)),
            "PING" => Some(Command::Ping),
            "NEXT" => Some(Command::Next),
            _ => line.strip_prefix("PLAY ").map(|path| Command::Play(PathBuf::from(path))),
        }
    }
}

/// Sends one command to a running engine. Fails (with a connection error)
/// if no engine is listening — the caller decides whether to start one.
pub fn send(command: &Command) -> io::Result<()> {
    imp::send(command.encode().as_bytes())
}

/// The engine's end. Non-blocking: `accept_command` returns `Ok(None)`
/// straight away when nobody is connecting, so it fits in each backend's
/// existing poll loop instead of needing its own thread.
pub struct Listener(imp::Listener);

impl Listener {
    pub fn bind() -> io::Result<Self> {
        imp::Listener::bind().map(Self)
    }

    /// The next command, if a client connected since the last call.
    /// A connection that sent garbage yields `Ok(None)` (and is logged),
    /// not an error: one bad client mustn't take the daemon down.
    pub fn accept_command(&mut self) -> io::Result<Option<Command>> {
        let Some(line) = self.0.accept_line()? else {
            return Ok(None);
        };
        // A client that connected and sent nothing (a probe) isn't an error.
        if line.trim().is_empty() {
            return Ok(None);
        }
        let command = Command::decode(&line);
        if command.is_none() {
            eprintln!("paperloom-engine: bad ipc command: {}", line.trim());
        }
        Ok(command)
    }
}

#[cfg(unix)]
mod imp {
    use std::io::{self, BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;

    /// Where the engine listens. `$XDG_RUNTIME_DIR` is the right place for
    /// a per-user, per-session socket (cleared on logout, not
    /// world-readable); falls back to the data dir if it's unset, which
    /// happens in some minimal environments and always on macOS.
    ///
    /// Inside Flatpak each `flatpak run` gets its own private runtime dir;
    /// only `$XDG_RUNTIME_DIR/app/<app id>` is shared between instances of
    /// the app (e.g. the engine started at login and a GUI opened later).
    pub fn socket_path() -> PathBuf {
        match std::env::var_os("XDG_RUNTIME_DIR") {
            Some(dir) if !dir.is_empty() => match std::env::var("FLATPAK_ID") {
                Ok(app_id) => PathBuf::from(dir).join("app").join(app_id).join("paperloom-engine.sock"),
                Err(_) => PathBuf::from(dir).join("paperloom-engine.sock"),
            },
            _ => crate::paths::library_dir().parent().expect("library_dir always has a parent").join("engine.sock"),
        }
    }

    pub fn send(bytes: &[u8]) -> io::Result<()> {
        UnixStream::connect(socket_path())?.write_all(bytes)
    }

    pub struct Listener(UnixListener);

    impl Listener {
        /// Clears out a stale socket left behind by a previous run that
        /// didn't exit cleanly (e.g. SIGKILL) — otherwise `bind` fails with
        /// `AddrInUse` even though nothing is actually listening anymore.
        pub fn bind() -> io::Result<Self> {
            let path = socket_path();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let _ = std::fs::remove_file(&path);
            let listener = UnixListener::bind(path)?;
            listener.set_nonblocking(true)?;
            Ok(Self(listener))
        }

        pub fn accept_line(&mut self) -> io::Result<Option<String>> {
            let stream = match self.0.accept() {
                Ok((stream, _addr)) => stream,
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                Err(err) => return Err(err),
            };
            // The accepted stream inherits non-blocking mode on some
            // platforms (macOS); the client writes its line right after
            // connecting, so a short blocking read is what we want.
            stream.set_nonblocking(false)?;
            stream.set_read_timeout(Some(std::time::Duration::from_secs(1)))?;
            let mut line = String::new();
            BufReader::new(stream).read_line(&mut line)?;
            Ok(Some(line))
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::io::{self, Write};
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING, HANDLE,
        INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{ReadFile, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_INBOUND};
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, SetNamedPipeHandleState, PIPE_NOWAIT, PIPE_READMODE_BYTE,
        PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
    };

    /// Per-user name, so two people logged into the same machine each get
    /// their own engine. The pipe's default security descriptor only gives
    /// write access to its creator (and admins/SYSTEM), so another user
    /// can't send commands to ours either.
    fn pipe_name() -> String {
        // `PAPERLOOM_IPC_NAME` gives tests their own pipe so they never meet a running engine.
        if let Ok(name) = std::env::var("PAPERLOOM_IPC_NAME") {
            return format!(r"\\.\pipe\paperloom-engine-{name}");
        }
        let user = std::env::var("USERNAME").unwrap_or_else(|_| "default".to_string());
        let user: String = user.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
        format!(r"\\.\pipe\paperloom-engine-{user}")
    }

    pub fn send(bytes: &[u8]) -> io::Result<()> {
        // std opens a named pipe's client end like any file. The server
        // only keeps one instance, so a second client arriving mid-command
        // gets ERROR_PIPE_BUSY; retrying briefly covers that.
        let mut last_error = None;
        for _ in 0..10 {
            match std::fs::OpenOptions::new().write(true).open(pipe_name()) {
                Ok(mut pipe) => return pipe.write_all(bytes),
                Err(err) if err.raw_os_error() == Some(231) => {
                    // ERROR_PIPE_BUSY
                    last_error = Some(err);
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(err) => return Err(err),
            }
        }
        Err(last_error.unwrap_or_else(|| io::Error::other("engine pipe busy")))
    }

    pub struct Listener {
        pipe: HANDLE,
    }

    impl Listener {
        /// `FILE_FLAG_FIRST_PIPE_INSTANCE` makes a second engine fail here
        /// instead of silently sharing the name with the first.
        pub fn bind() -> io::Result<Self> {
            let name: Vec<u16> = std::ffi::OsStr::new(&pipe_name()).encode_wide().chain(Some(0)).collect();
            // SAFETY: `name` is a valid NUL-terminated wide string that
            // outlives the call; null security attributes = default DACL.
            let pipe = unsafe {
                CreateNamedPipeW(
                    name.as_ptr(),
                    PIPE_ACCESS_INBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    1,
                    0,
                    4096,
                    0,
                    std::ptr::null(),
                )
            };
            if pipe == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { pipe })
        }

        /// In `PIPE_NOWAIT` mode `ConnectNamedPipe` returns at once:
        /// `ERROR_PIPE_CONNECTED` means a client is there, `ERROR_PIPE_LISTENING`
        /// means nobody yet. Once connected, the pipe is switched to blocking
        /// mode just long enough to read the client's one line, then
        /// disconnected and put back to listening.
        pub fn accept_line(&mut self) -> io::Result<Option<String>> {
            // SAFETY: `self.pipe` is a live pipe handle owned by `self`.
            let connected = unsafe { ConnectNamedPipe(self.pipe, std::ptr::null_mut()) } != 0;
            if !connected {
                // SAFETY: plain thread-local error query.
                match unsafe { GetLastError() } {
                    ERROR_PIPE_CONNECTED => {}
                    ERROR_PIPE_LISTENING => return Ok(None),
                    // A client wrote its command and closed its end between two
                    // polls. The bytes are still in the pipe and readable below;
                    // disconnecting here would throw the command away.
                    ERROR_NO_DATA => {}
                    code => return Err(io::Error::from_raw_os_error(code as i32)),
                }
            }

            self.set_mode(PIPE_READMODE_BYTE | PIPE_WAIT);
            let line = self.read_until_newline();
            self.disconnect();
            self.set_mode(PIPE_READMODE_BYTE | PIPE_NOWAIT);
            line.map(Some)
        }

        fn read_until_newline(&self) -> io::Result<String> {
            let mut bytes = Vec::new();
            let mut buf = [0u8; 512];
            while !bytes.contains(&b'\n') && bytes.len() < 64 * 1024 {
                let mut read = 0u32;
                // SAFETY: `buf` is valid for `buf.len()` bytes; synchronous read.
                let ok = unsafe { ReadFile(self.pipe, buf.as_mut_ptr(), buf.len() as u32, &mut read, std::ptr::null_mut()) } != 0;
                if !ok {
                    // SAFETY: plain thread-local error query.
                    let code = unsafe { GetLastError() };
                    if code == ERROR_BROKEN_PIPE {
                        break; // client closed after writing: normal end
                    }
                    return Err(io::Error::from_raw_os_error(code as i32));
                }
                if read == 0 {
                    break;
                }
                bytes.extend_from_slice(&buf[..read as usize]);
            }
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }

        fn set_mode(&self, mode: u32) {
            // SAFETY: `self.pipe` is live; `mode` outlives the call.
            unsafe { SetNamedPipeHandleState(self.pipe, &mode, std::ptr::null(), std::ptr::null()) };
        }

        fn disconnect(&self) {
            // SAFETY: `self.pipe` is live.
            unsafe { DisconnectNamedPipe(self.pipe) };
        }
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            // SAFETY: closing the handle we own, exactly once.
            unsafe { CloseHandle(self.pipe) };
        }
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
    fn windows_path_round_trips() {
        let cmd = Command::Play(PathBuf::from(r"C:\Users\Me\Videos\my loop.mp4"));
        assert_eq!(Command::decode(&cmd.encode()), Some(cmd));
    }

    #[test]
    fn stop_and_quit_round_trip() {
        assert_eq!(Command::decode(&Command::Stop.encode()), Some(Command::Stop));
        assert_eq!(Command::decode(&Command::Quit.encode()), Some(Command::Quit));
        assert_eq!(Command::decode(&Command::Pause.encode()), Some(Command::Pause));
        assert_eq!(Command::decode(&Command::Resume.encode()), Some(Command::Resume));
        assert_eq!(Command::decode("STOP\r\n"), Some(Command::Stop));
    }

    /// The real transport: bind, send from a client, accept non-blockingly.
    #[cfg(unix)]
    #[test]
    fn unix_listener_round_trips() {
        let dir = std::env::temp_dir().join(format!("paperloom-ipc-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Only this test reads XDG_RUNTIME_DIR in this process.
        std::env::set_var("XDG_RUNTIME_DIR", &dir);
        std::env::remove_var("FLATPAK_ID");

        let mut listener = Listener::bind().unwrap();
        assert_eq!(listener.accept_command().unwrap(), None, "nothing pending yet");

        let play = Command::Play(PathBuf::from("/tmp/a b.mp4"));
        send(&play).unwrap();
        assert_eq!(listener.accept_command().unwrap(), Some(play));
        send(&Command::Quit).unwrap();
        assert_eq!(listener.accept_command().unwrap(), Some(Command::Quit));

        drop(listener);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn garbage_decodes_to_none() {
        assert_eq!(Command::decode("not a command\n"), None);
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    /// A client that writes its command and closes before the engine's next
    /// poll must still be heard (the command used to be dropped).
    #[test]
    fn a_command_from_a_client_that_already_left_is_delivered() {
        std::env::set_var("PAPERLOOM_IPC_NAME", format!("test-{}", std::process::id()));
        let mut listener = Listener::bind().expect("pipe binds");
        send(&Command::Next).expect("client sends");
        std::thread::sleep(std::time::Duration::from_millis(300)); // the engine polls ~5 times a second
        assert_eq!(listener.accept_command().unwrap(), Some(Command::Next));
    }
}
