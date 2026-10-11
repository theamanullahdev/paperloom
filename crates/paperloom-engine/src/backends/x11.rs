//! X11 desktop-layer window + libmpv embedding.
//!
//! The technique (same idea as `xwinwrap`): create a plain top-level window
//! covering the whole screen, tag it `_NET_WM_WINDOW_TYPE_DESKTOP` so a
//! compliant window manager (Muffin/Mutter, which Cinnamon is built on)
//! stacks it below every normal window and leaves it undecorated and
//! unfocused, then hand its X11 window ID to libmpv via `wid` so mpv renders
//! directly into it.
//!
//! This process stays running as a daemon: after setting up the window and
//! an initial file (if one was passed on the CLI), it listens on the IPC
//! socket (see `paperloom_common::ipc`) for `paperloom-gui` to tell it what
//! to play next.

use std::error::Error;
use std::io::{BufRead, BufReader};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::time::Duration;

use libmpv2::Mpv;
use paperloom_common::ipc::Command;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt as _, CreateWindowAux, EventMask, PropMode, WindowClass};
use x11rb::wrapper::ConnectionExt as _;

use crate::pause::PauseDetector;

pub fn run(initial_path: Option<&str>) -> Result<(), Box<dyn Error>> {
    // Fail loudly before touching the display: a bad path otherwise shows
    // up as a silent black screen (mpv accepts `loadfile` synchronously
    // and only fails to actually play asynchronously), which is confusing
    // when running this directly during development. Doesn't catch every
    // failure mode (corrupt file, unsupported codec) — only "no such file".
    if let Some(path) = initial_path {
        if !Path::new(path).is_file() {
            return Err(format!("no such video file: {path}").into());
        }
    }

    let (conn, screen_num) = x11rb::connect(None)?;
    let screen = conn.setup().roots[screen_num].clone();

    let window = conn.generate_id()?;
    let aux = CreateWindowAux::new()
        .background_pixel(screen.black_pixel)
        .event_mask(EventMask::EXPOSURE | EventMask::STRUCTURE_NOTIFY);

    conn.create_window(
        screen.root_depth,
        window,
        screen.root,
        0,
        0,
        screen.width_in_pixels,
        screen.height_in_pixels,
        0,
        WindowClass::INPUT_OUTPUT,
        screen.root_visual,
        &aux,
    )?;

    mark_as_desktop_window(&conn, window)?;

    conn.map_window(window)?;
    conn.flush()?;

    let mpv = Mpv::with_initializer(|init| {
        init.set_property("wid", window as i64)?;
        Ok(())
    })?;

    // Wallpaper defaults: loops forever, no on-screen controller, doesn't
    // try to grab keyboard/mouse input.
    mpv.set_property("loop-file", "inf".to_string())?;
    mpv.set_property("osc", false)?;
    mpv.set_property("input-default-bindings", false)?;
    mpv.set_property("input-vo-keyboard", false)?;
    mpv.set_property("cursor-autohide", "no".to_string())?;
    // A wallpaper should fill the screen like CSS `background-size: cover`,
    // not letterbox/pillarbox like a video player when the clip's aspect
    // ratio doesn't match the display's (e.g. a 16:9 clip on a 16:10
    // screen). panscan=1.0 tells mpv to crop-to-fill instead of showing
    // black bars.
    mpv.set_property("panscan", 1.0)?;
    apply_mute_from_settings(&mpv)?;

    if let Some(path) = initial_path {
        mpv.command("loadfile", &[path])?;
    }

    let listener = bind_ipc_socket()?;
    listener.set_nonblocking(true)?;

    let pause_detector = PauseDetector::new(&conn, screen.root)?;
    let mut is_paused = false;
    // Pause triggers are checked every 10 ticks (~2s at the 200ms tick
    // below), not every tick: two of the four checks shell out to
    // `gdbus`/`powerprofilesctl`, and none of lock/battery-saver/fullscreen
    // state changes fast enough to need checking faster than that.
    let mut tick: u32 = 0;

    loop {
        while conn.poll_for_event()?.is_some() {}

        match listener.accept() {
            Ok((stream, _addr)) => handle_connection(stream, &mpv),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(err) => eprintln!("paperloom-engine: ipc accept error: {err}"),
        }

        tick += 1;
        if tick.is_multiple_of(10) {
            let settings = paperloom_common::load();
            let should_be_paused =
                pause_detector.should_pause(&conn, screen.width_in_pixels, screen.height_in_pixels, window, &settings);
            if should_be_paused != is_paused {
                is_paused = should_be_paused;
                if let Err(err) = mpv.set_property("pause", is_paused) {
                    eprintln!("paperloom-engine: failed to set pause state: {err}");
                }
            }
        }

        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Binds the IPC socket, clearing out a stale one left behind by a
/// previous run that didn't exit cleanly (e.g. SIGKILL) — otherwise
/// `bind` fails with `AddrInUse` even though nothing is actually
/// listening anymore.
fn bind_ipc_socket() -> std::io::Result<UnixListener> {
    let path = paperloom_common::ipc::socket_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _ = std::fs::remove_file(&path);
    UnixListener::bind(path)
}

fn handle_connection(stream: UnixStream, mpv: &Mpv) {
    let mut line = String::new();
    if let Err(err) = BufReader::new(stream).read_line(&mut line) {
        eprintln!("paperloom-engine: ipc read error: {err}");
        return;
    }

    match Command::decode(&line) {
        Some(Command::Play(path)) => {
            if !path.is_file() {
                eprintln!("paperloom-engine: ipc Play for missing file: {}", path.display());
                return;
            }
            // Re-read settings on every Play so a mute change takes effect
            // on the next wallpaper without needing a restart.
            if let Err(err) = apply_mute_from_settings(mpv) {
                eprintln!("paperloom-engine: failed to apply settings: {err}");
            }
            let path = path.to_string_lossy().into_owned();
            if let Err(err) = mpv.command("loadfile", &[path.as_str()]) {
                eprintln!("paperloom-engine: failed to play {path}: {err}");
            }
        }
        Some(Command::Stop) => {
            if let Err(err) = mpv.command("stop", &[]) {
                eprintln!("paperloom-engine: failed to stop: {err}");
            }
        }
        None => eprintln!("paperloom-engine: bad ipc command: {}", line.trim()),
    }
}

fn apply_mute_from_settings(mpv: &Mpv) -> Result<(), Box<dyn Error>> {
    let settings = paperloom_common::load();
    mpv.set_property("mute", settings.muted)?;
    Ok(())
}

fn mark_as_desktop_window(conn: &impl Connection, window: u32) -> Result<(), Box<dyn Error>> {
    let net_wm_window_type = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE")?.reply()?.atom;
    let net_wm_window_type_desktop = conn
        .intern_atom(false, b"_NET_WM_WINDOW_TYPE_DESKTOP")?
        .reply()?
        .atom;

    conn.change_property32(
        PropMode::REPLACE,
        window,
        net_wm_window_type,
        x11rb::protocol::xproto::AtomEnum::ATOM,
        &[net_wm_window_type_desktop],
    )?;

    Ok(())
}
