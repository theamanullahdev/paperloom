//! "Only one of me": starting Paperloom while it is already open must bring
//! the open window forward instead of making a second one, and the tray
//! widget closes itself when its icon is clicked again.
//!
//! Each named instance listens on a small Unix socket. A second launch
//! connects, sends one line (what it wanted: "SHOW", "PAGE usage", …) and
//! exits; the first instance receives that line as a message. Unix only —
//! elsewhere every launch simply runs (the previous behaviour).

use std::sync::Mutex;

use iced::futures::channel::mpsc::UnboundedReceiver;
use iced::futures::stream::{BoxStream, StreamExt};

static INBOX: Mutex<Option<UnboundedReceiver<String>>> = Mutex::new(None);

/// Becomes the one running instance called `name`, or — if one already
/// runs — tells it `message` and returns `false` (the caller should exit).
pub fn claim(name: &str, message: &str) -> bool {
    imp::claim(name, message)
}

/// Gives up being the running instance called `name` (restarting into a new
/// copy), so the next launch starts fresh instead of talking to this one.
pub fn release(name: &str) {
    imp::release(name);
}

/// What later launches asked for, as a stream of lines. Take it once; the
/// subscription that owns it keeps it for the life of the process.
pub fn requests() -> BoxStream<'static, String> {
    match INBOX.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take() {
        Some(receiver) => receiver.boxed(),
        None => iced::futures::stream::empty().boxed(),
    }
}

#[cfg(unix)]
mod imp {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;

    use iced::futures::channel::mpsc::unbounded;

    use super::INBOX;

    fn socket_path(name: &str) -> PathBuf {
        match std::env::var_os("XDG_RUNTIME_DIR") {
            Some(dir) => PathBuf::from(dir).join(format!("paperloom-{name}.sock")),
            // No runtime dir (rare): a per-user name in the temp folder.
            None => std::env::temp_dir().join(format!("paperloom-{name}-{}.sock", std::env::var("USER").unwrap_or_default())),
        }
    }

    pub fn release(name: &str) {
        let _ = std::fs::remove_file(socket_path(name));
    }

    pub fn claim(name: &str, message: &str) -> bool {
        let path = socket_path(name);

        if let Ok(mut stream) = UnixStream::connect(&path) {
            let _ = stream.write_all(format!("{message}\n").as_bytes());
            return false;
        }

        // Nobody answered: either we're first, or a crashed instance left
        // its socket file behind. Clear it and listen.
        let _ = std::fs::remove_file(&path);
        let Ok(listener) = UnixListener::bind(&path) else { return true };

        let (sender, receiver) = unbounded();
        *INBOX.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(receiver);
        std::thread::Builder::new()
            .name(format!("paperloom-{name}-instance"))
            .spawn(move || {
                // Blocking accept: sleeps in the kernel until another launch
                // connects, so waiting costs nothing.
                for stream in listener.incoming().flatten() {
                    let mut line = String::new();
                    if BufReader::new(stream).read_line(&mut line).is_ok() && sender.unbounded_send(line.trim().to_string()).is_err() {
                        break;
                    }
                }
            })
            .ok();
        true
    }
}

#[cfg(not(unix))]
mod imp {
    pub fn release(_name: &str) {}

    pub fn claim(_name: &str, _message: &str) -> bool {
        true
    }
}
