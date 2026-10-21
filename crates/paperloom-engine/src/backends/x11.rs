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
//! an initial file (the CLI path, or the last wallpaper), it listens on the IPC
//! socket (see `paperloom_common::ipc`) for `paperloom-gui` to tell it what
//! to play next.

use std::error::Error;
use std::time::Duration;

use libmpv2::Mpv;
use paperloom_common::ipc::Listener;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ConfigureWindowAux, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, StackMode, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;

use crate::pause::PauseDetector;
use crate::player::{self, Flow};

pub fn run(initial_path: Option<&str>) -> Result<(), Box<dyn Error>> {
    player::validate_cli_path(initial_path)?;

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
    lower_below_everything(&conn, window)?;
    conn.flush()?;

    let mpv = Mpv::with_initializer(|init| {
        init.set_property("wid", window as i64)?;
        Ok(())
    })?;

    player::configure_wallpaper(&mpv)?;

    if let Some(path) = player::startup_wallpaper(initial_path) {
        player::play(&mpv, &path);
    }

    let mut listener = Listener::bind()?;

    let pause_detector = PauseDetector::new(&conn, screen.root)?;
    let mut hotkeys = crate::hotkeys_x11::Hotkeys::new(screen.root);
    let mut is_paused = false;
    // Pause triggers are checked every 10 ticks (~2s at the 200ms tick
    // below), not every tick: two of the four checks shell out to
    // `gdbus`/`upower`, and none of lock/battery/fullscreen state changes
    // fast enough to need checking faster than that.
    let mut tick: u32 = 0;

    loop {
        while let Some(event) = conn.poll_for_event()? {
            if let Some(shortcut) = hotkeys.event(&event) {
                player::shortcut(&mpv, shortcut);
            }
        }

        if player::poll_ipc(&mut listener, &mpv) == Flow::Quit {
            return Ok(());
        }

        tick += 1;
        if tick.is_multiple_of(10) {
            let settings = paperloom_common::load();
            player::periodic(&settings, &mpv);
            hotkeys.sync(&conn, &settings.hotkey_next, &settings.hotkey_pause);
            let should_be_paused =
                pause_detector.should_pause(&conn, screen.width_in_pixels, screen.height_in_pixels, window, &settings);
            if should_be_paused != is_paused {
                is_paused = should_be_paused;
                player::set_paused(&mpv, is_paused);
            }

            // Nemo's desktop-icons window is also tagged `_NET_WM_WINDOW_TYPE_DESKTOP`,
            // and EWMH doesn't define a stacking order *within* that type — so on some
            // session restarts Nemo (re)creates or restacks its icon window above ours,
            // leaving icons buried under the video. Re-asserting "below everything" on
            // the same low-frequency tick as the pause check keeps this corrected
            // without adding a dedicated poll loop.
            if let Err(err) = lower_below_everything(&conn, window) {
                eprintln!("paperloom-engine: failed to re-lower desktop window: {err}");
            }
        }

        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Explicitly stacks `window` below every other window, including Nemo's
/// desktop-icons window — both are `_NET_WM_WINDOW_TYPE_DESKTOP`, and
/// nothing in EWMH orders windows that share that type, so without this
/// the wallpaper can end up drawn *over* the desktop icons instead of
/// behind them.
fn lower_below_everything(conn: &impl Connection, window: u32) -> Result<(), Box<dyn Error>> {
    conn.configure_window(window, &ConfigureWindowAux::new().stack_mode(StackMode::BELOW))?;
    conn.flush()?;
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

    // On every workspace, not just the one the engine started on — without
    // this, switching workspace showed the plain system background.
    // `_NET_WM_DESKTOP = 0xFFFFFFFF` is EWMH's "all desktops"; STICKY says
    // the same through the state list, and the SKIP_* states keep the
    // window out of the taskbar, Alt+Tab and pagers.
    let atom = |name: &[u8]| -> Result<u32, Box<dyn Error>> { Ok(conn.intern_atom(false, name)?.reply()?.atom) };
    conn.change_property32(PropMode::REPLACE, window, atom(b"_NET_WM_DESKTOP")?, AtomEnum::CARDINAL, &[0xFFFF_FFFF])?;
    conn.change_property32(
        PropMode::REPLACE,
        window,
        atom(b"_NET_WM_STATE")?,
        AtomEnum::ATOM,
        &[atom(b"_NET_WM_STATE_STICKY")?, atom(b"_NET_WM_STATE_SKIP_TASKBAR")?, atom(b"_NET_WM_STATE_SKIP_PAGER")?, atom(b"_NET_WM_STATE_BELOW")?],
    )?;

    Ok(())
}
