//! Full-screen preview of a wallpaper, started by the GUI on double-click
//! (`paperloom-engine --preview <file>`).
//!
//! Unlike the real wallpaper window this is an ordinary, focused,
//! full-screen window on top of everything, so the user can judge the
//! video before committing; any key, a click, or closing the window ends
//! it. It's X11 only on purpose — on a Wayland session it runs through
//! XWayland, which every current Wayland desktop provides, so one code
//! path covers both.

use std::error::Error;
use std::path::Path;
use std::time::Duration;

use libmpv2::Mpv;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, WindowClass};
use x11rb::protocol::Event;
use x11rb::wrapper::ConnectionExt as _;

pub fn run(path: &str) -> Result<(), Box<dyn Error>> {
    if !Path::new(path).is_file() {
        return Err(format!("no such video file: {path}").into());
    }

    let (conn, screen_num) = x11rb::connect(None)?;
    let screen = conn.setup().roots[screen_num].clone();

    let window = conn.generate_id()?;
    let aux = CreateWindowAux::new()
        .background_pixel(screen.black_pixel)
        .event_mask(EventMask::KEY_PRESS | EventMask::BUTTON_PRESS | EventMask::STRUCTURE_NOTIFY);
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

    let atom = |name: &[u8]| -> Result<u32, Box<dyn Error>> { Ok(conn.intern_atom(false, name)?.reply()?.atom) };
    let wm_state = atom(b"_NET_WM_STATE")?;
    let fullscreen = atom(b"_NET_WM_STATE_FULLSCREEN")?;
    let wm_protocols = atom(b"WM_PROTOCOLS")?;
    let wm_delete = atom(b"WM_DELETE_WINDOW")?;

    // Fullscreen is requested via the property before mapping (the WM
    // honours it on map); the delete protocol makes the window manager's
    // close button send us a message instead of killing the connection.
    conn.change_property32(PropMode::REPLACE, window, wm_state, AtomEnum::ATOM, &[fullscreen])?;
    conn.change_property32(PropMode::REPLACE, window, wm_protocols, AtomEnum::ATOM, &[wm_delete])?;
    conn.change_property8(PropMode::REPLACE, window, AtomEnum::WM_NAME, AtomEnum::STRING, b"Paperloom preview")?;
    conn.map_window(window)?;
    conn.flush()?;

    let mpv = Mpv::with_initializer(|init| {
        init.set_property("wid", window as i64)?;
        Ok(())
    })?;
    mpv.set_property("loop-file", "inf".to_string())?;
    mpv.set_property("osc", false)?;
    mpv.set_property("input-default-bindings", false)?;
    mpv.set_property("input-vo-keyboard", false)?;
    // Keep pointer/keyboard events on our window rather than mpv's child.
    mpv.set_property("input-cursor", false)?;
    mpv.set_property("cursor-autohide", "always".to_string())?;
    mpv.set_property("mute", true)?;
    mpv.set_property("hwdec", if paperloom_common::load().hardware_decoding { "auto-safe" } else { "no" }.to_string())?;
    // Same crop-to-fill as the real wallpaper, so the preview shows what
    // the desktop will show.
    mpv.set_property("panscan", 1.0)?;
    mpv.command("loadfile", &[path])?;

    loop {
        while let Some(event) = conn.poll_for_event()? {
            match event {
                Event::KeyPress(_) | Event::ButtonPress(_) | Event::DestroyNotify(_) | Event::ClientMessage(_) => return Ok(()),
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
