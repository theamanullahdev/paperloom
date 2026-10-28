//! X11: a 32-bit (ARGB) window of the desktop type. It needs a compositing
//! window manager for transparency (Muffin, Mutter, KWin, Xfwm with the
//! compositor on); without one the corners are drawn solid.

use std::error::Error;
use std::time::{Duration, Instant};

use tiny_skia::Pixmap;
use x11rb::connection::Connection;
use x11rb::protocol::shape::{self, ConnectionExt as _, SK};
use x11rb::protocol::xproto::{
    AtomEnum, ColormapAlloc, ConfigureWindowAux, ConnectionExt as _, CreateWindowAux, EventMask, ImageFormat, PropMode, StackMode, VisualClass, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;

use paperloom_common::widgets::{Placed, Widgets};

use crate::control::Control;
use crate::render::Text;
use crate::widgets::{self, Context};

struct Window {
    id: u32,
    placed: Placed,
    pixmap: Option<Pixmap>,
    /// When what it shows changes next.
    due: Instant,
}

pub fn run(widgets: &Widgets, text: &mut Text, control: &mut Control) -> Result<(), Box<dyn Error>> {
    let (conn, screen_num) = x11rb::connect(None)?;
    let screen = conn.setup().roots[screen_num].clone();
    let (depth, visual) = argb_visual(&screen).ok_or("this X server has no 32-bit visual, widgets can't be transparent")?;
    let colormap = conn.generate_id()?;
    conn.create_colormap(ColormapAlloc::NONE, colormap, screen.root, visual)?;

    let mut windows: Vec<Window> = Vec::new();
    for placed in widgets.placed.iter().cloned() {
        let (w, h) = widgets::size_of(&placed.kind);
        let (w, h) = ((w as f32 * placed.scale) as u16, (h as f32 * placed.scale) as u16);
        let x = (placed.x * f32::from(screen.width_in_pixels)) as i16;
        let y = (placed.y * f32::from(screen.height_in_pixels)) as i16;
        let id = conn.generate_id()?;
        conn.create_window(
            depth,
            id,
            screen.root,
            x,
            y,
            w,
            h,
            0,
            WindowClass::INPUT_OUTPUT,
            visual,
            &CreateWindowAux::new().border_pixel(0).colormap(colormap).event_mask(EventMask::EXPOSURE),
        )?;
        mark_as_desktop_widget(&conn, id, &placed)?;
        conn.map_window(id)?;
        stack_below_icons(&conn, screen.root, id)?;
        windows.push(Window { id, placed, pixmap: None, due: Instant::now() });
    }
    conn.flush()?;

    let gc = conn.generate_id()?;
    if let Some(first) = windows.first() {
        conn.create_gc(gc, first.id, &Default::default())?;
    }

    while !control.changed() {
        if control.paused() {
            std::thread::sleep(Duration::from_secs(1));
            continue;
        }
        let now = Instant::now();
        control.refresh_sample();
        let slow = control.slow();
        for window in &mut windows {
            if window.due > now && window.pixmap.is_some() {
                continue;
            }
            let Some(frame) = widgets::render(&window.placed.kind, window.placed.scale, text, &Context { sample: &control.sample, slow }) else {
                window.due = now + Duration::from_secs(3600);
                continue;
            };
            present(&conn, depth, window.id, gc, &frame.pixmap)?;
            window.due = now + frame.next_change;
            window.pixmap = Some(frame.pixmap);
        }
        conn.flush()?;

        // Sleep until the next widget is due, answering events (a repaint
        // request after a compositor restart) in at most half a second.
        let wake = windows.iter().map(|window| window.due).min().unwrap_or(now + Duration::from_secs(1));
        let nap = wake.saturating_duration_since(Instant::now()).min(Duration::from_secs(1));
        std::thread::sleep(nap);
        while let Some(event) = conn.poll_for_event()? {
            if let x11rb::protocol::Event::Expose(expose) = event {
                if let Some(window) = windows.iter().find(|window| window.id == expose.window) {
                    if let Some(pixmap) = &window.pixmap {
                        present(&conn, depth, window.id, gc, pixmap)?;
                    }
                }
            }
        }
        conn.flush()?;
    }
    Ok(())
}

fn argb_visual(screen: &x11rb::protocol::xproto::Screen) -> Option<(u8, u32)> {
    screen
        .allowed_depths
        .iter()
        .filter(|depth| depth.depth == 32)
        .flat_map(|depth| depth.visuals.iter())
        .find(|visual| visual.class == VisualClass::TRUE_COLOR)
        .map(|visual| (32, visual.visual_id))
}

fn atom(conn: &impl Connection, name: &[u8]) -> Result<u32, Box<dyn Error>> {
    Ok(conn.intern_atom(false, name)?.reply()?.atom)
}

fn mark_as_desktop_widget(conn: &impl Connection, window: u32, placed: &Placed) -> Result<(), Box<dyn Error>> {
    conn.change_property32(PropMode::REPLACE, window, atom(conn, b"_NET_WM_WINDOW_TYPE")?, AtomEnum::ATOM, &[atom(conn, b"_NET_WM_WINDOW_TYPE_DESKTOP")?])?;
    // Every workspace, and out of the taskbar, Alt+Tab and pagers.
    conn.change_property32(PropMode::REPLACE, window, atom(conn, b"_NET_WM_DESKTOP")?, AtomEnum::CARDINAL, &[0xFFFF_FFFF])?;
    conn.change_property32(
        PropMode::REPLACE,
        window,
        atom(conn, b"_NET_WM_STATE")?,
        AtomEnum::ATOM,
        &[atom(conn, b"_NET_WM_STATE_STICKY")?, atom(conn, b"_NET_WM_STATE_SKIP_TASKBAR")?, atom(conn, b"_NET_WM_STATE_SKIP_PAGER")?],
    )?;
    conn.change_property8(PropMode::REPLACE, window, AtomEnum::WM_NAME, AtomEnum::STRING, b"Paperloom widget")?;
    // Never takes focus.
    conn.change_property32(PropMode::REPLACE, window, AtomEnum::WM_HINTS, AtomEnum::WM_HINTS, &[1, 0, 0, 0, 0, 0, 0, 0, 0])?;
    if placed.click_through {
        // An empty input shape: every click goes to what is underneath.
        conn.shape_rectangles(shape::SO::SET, SK::INPUT, x11rb::protocol::xproto::ClipOrdering::UNSORTED, window, 0, 0, &[])?;
    }
    Ok(())
}

/// Puts the widget just below the desktop-icons window (Nemo's), which is
/// also a desktop-type window, so it stays above the wallpaper — the
/// wallpaper engine keeps itself at the very bottom.
fn stack_below_icons(conn: &impl Connection, root: u32, window: u32) -> Result<(), Box<dyn Error>> {
    let wm_class = AtomEnum::WM_CLASS;
    for child in conn.query_tree(root)?.reply()?.children {
        if child == window {
            continue;
        }
        let Ok(reply) = conn.get_property(false, child, wm_class, AtomEnum::STRING, 0, 64)?.reply() else { continue };
        let class = String::from_utf8_lossy(&reply.value).to_lowercase();
        if class.contains("nemo-desktop") || class.contains("desktop_window") || class.contains("nautilus-desktop") {
            conn.configure_window(window, &ConfigureWindowAux::new().sibling(child).stack_mode(StackMode::BELOW))?;
            return Ok(());
        }
    }
    Ok(())
}

/// Sends a premultiplied RGBA pixmap as BGRA, which is what a 32-bit
/// little-endian X window expects.
fn present(conn: &impl Connection, depth: u8, window: u32, gc: u32, pixmap: &Pixmap) -> Result<(), Box<dyn Error>> {
    let mut bytes = pixmap.data().to_vec();
    for pixel in bytes.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    conn.put_image(ImageFormat::Z_PIXMAP, window, gc, pixmap.width() as u16, pixmap.height() as u16, 0, 0, 0, depth, &bytes)?;
    Ok(())
}
