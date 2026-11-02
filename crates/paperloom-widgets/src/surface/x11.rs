//! X11: a 32-bit (ARGB) window per widget, of the dock type with the "below"
//! state. That puts it in the window manager's bottom layer: above the
//! wallpaper and the desktop icons (the desktop layer, whose inner order no
//! standard defines and that a window manager may reshuffle at any time),
//! below every normal window, and out of Alt+Tab, the taskbar and "show
//! desktop". Because it is a layer, not a position among siblings, nothing
//! has to be re-asserted when the wallpaper window is replaced. It needs a
//! compositing window manager for transparency (Muffin, Mutter, KWin, Xfwm
//! with the compositor on); without one the corners are drawn solid.


use std::error::Error;
use std::time::{Duration, Instant};

use tiny_skia::Pixmap;
use x11rb::connection::Connection;
use x11rb::protocol::shape::{self, ConnectionExt as _, SK};
use x11rb::protocol::xproto::{
    AtomEnum, ColormapAlloc, ConfigureWindowAux, ConnectionExt as _, CreateWindowAux, EventMask, GrabMode, ImageFormat, PropMode, StackMode, VisualClass,
    WindowClass,
};
use x11rb::protocol::xproto::{ExposeEvent, MapNotifyEvent, VisibilityNotifyEvent};
use x11rb::protocol::Event;
use std::os::fd::AsRawFd;
use x11rb::wrapper::ConnectionExt as _;

use paperloom_common::widgets::{Placed, Widgets};

use crate::control::Control;
use crate::interact;
use crate::render::menu::{self, Choice};
use crate::render::Text;
use crate::widgets::{self, Context};

/// Closer than this (pixels) and a press-and-release is a click, not a drag.
const DRAG_SLOP: i16 = 3;
/// How long the right-click menu stays open untouched.
const MENU_TIMEOUT: Duration = Duration::from_secs(15);

struct Drag {
    /// Pointer and window positions when the button went down.
    root: (i16, i16),
    from: (i16, i16),
    moved: bool,
}

/// The open right-click menu: its own small window, so it appears at once and
/// never resizes the widget. One at a time; the pointer is grabbed while it is
/// open, so a click anywhere else closes it.
struct Popup {
    id: u32,
    /// The widget it belongs to.
    widget: u32,
    items: Vec<(Choice, &'static str)>,
    hover: Option<usize>,
    /// A menu left open closes by itself (it holds the pointer while open).
    opened: Instant,
}

/// What opening a menu window needs.
struct Shared {
    root: u32,
    depth: u8,
    visual: u32,
    colormap: u32,
    gc: u32,
    screen: (u16, u16),
}

struct Window {
    id: u32,
    placed: Placed,
    /// Where the widget is now (top-left, pixels) and how big it is.
    at: (i16, i16),
    size: (u16, u16),
    /// What the widget itself shows (kept so it can be redrawn after the menu).
    pixmap: Option<Pixmap>,
    /// When what it shows changes next.
    due: Instant,
    drag: Option<Drag>,
}

pub fn run(widgets: &Widgets, text: &mut Text, control: &mut Control) -> Result<(), Box<dyn Error>> {
    let (conn, screen_num) = x11rb::connect(None)?;
    let screen = conn.setup().roots[screen_num].clone();
    let (screen_w, screen_h) = (screen.width_in_pixels, screen.height_in_pixels);
    let (depth, visual) = argb_visual(&screen).ok_or("this X server has no 32-bit visual, widgets can't be transparent")?;
    let colormap = conn.generate_id()?;
    conn.create_colormap(ColormapAlloc::NONE, colormap, screen.root, visual)?;

    let mut windows: Vec<Window> = Vec::new();
    for placed in widgets.placed.iter().cloned() {
        windows.push(make_window(&conn, &screen, depth, visual, colormap, placed)?);
    }
    conn.flush()?;

    let gc = conn.generate_id()?;
    if let Some(first) = windows.first() {
        conn.create_gc(gc, first.id, &Default::default())?;
    }
    paperloom_common::widgets::Status::update(|status| {
        status.backend = "x11".into();
        status.verified = true;
        status.windows = windows.len() as u32;
    });
    let shared = Shared { root: screen.root, depth, visual, colormap, gc, screen: (screen_w, screen_h) };
    let mut popup: Option<Popup> = None;
    // Whatever the window manager or the graphics driver does to a window's contents, they are
    // sent again every so often (a few small pictures; nothing else is redrawn).
    let mut last_repaint = Instant::now();

    loop {
        if control.changed() {
            // Only what changed is touched: a moved widget moves, a removed one goes, a new one appears.
            if control.test_over() || control.testing() {
                break;
            }
            let new = Widgets::load();
            if !new.enabled || new.placed.is_empty() {
                break;
            }
            apply_change(&conn, &screen, depth, visual, colormap, &mut windows, &new)?;
            control.mark_seen();
            control.sample.set_needs(new.placed.iter().map(|placed| placed.kind.as_str()));
            let has = |kind: &str| new.placed.iter().any(|placed| placed.kind == kind);
            crate::feeds::set_needs(has("weather"), has("nowplaying"));
            paperloom_common::widgets::Status::update(|status| status.windows = windows.len() as u32);
        }
        if control.paused() {
            // Nothing may hold the pointer while the widgets are paused (a
            // fullscreen game, the lock screen): close the menu, drop any drag.
            close_popup(&conn, &mut popup)?;
            for window in &mut windows {
                window.drag = None;
            }
            conn.flush()?;
            wait(conn.stream(), Duration::from_secs(1));
            while conn.poll_for_event()?.is_some() {}
            continue;
        }
        let now = Instant::now();
        if now.duration_since(last_repaint) >= Duration::from_secs(20) {
            last_repaint = now;
            for window in &windows {
                if let (Some(pixmap), None) = (&window.pixmap, &window.drag) {
                    present(&conn, depth, window.id, gc, pixmap)?;
                }
            }
        }
        control.refresh_sample();
        let slow = control.slow();
        for window in &mut windows {
            if window.due > now && window.pixmap.is_some() {
                continue;
            }
            let Some(frame) = widgets::render(&window.placed, text, &Context { sample: &control.sample, slow }) else {
                window.due = now + Duration::from_secs(3600);
                continue;
            };
            window.due = now + frame.next_change;
            let Some(pixmap) = frame.pixmap else { continue };
            if window.drag.is_none() || window.pixmap.is_none() {
                present(&conn, depth, window.id, gc, &pixmap)?;
            }
            window.pixmap = Some(pixmap);
        }
        conn.flush()?;

        while let Some(event) = conn.poll_for_event()? {
            handle(&conn, event, &mut windows, &mut popup, &shared, text, control)?;
        }
        if popup.as_ref().is_some_and(|open| open.opened.elapsed() > MENU_TIMEOUT) {
            close_popup(&conn, &mut popup)?;
        }
        conn.flush()?;

        // Sleep until the next widget is due. While something is being dragged
        // or a menu is open, stay responsive.
        let busy = popup.is_some() || windows.iter().any(|window| window.drag.is_some());
        let wake = windows.iter().map(|window| window.due).min().unwrap_or(now + Duration::from_secs(1));
        let nap = if busy { Duration::from_millis(15) } else { wake.saturating_duration_since(Instant::now()).min(Duration::from_secs(1)) };
        wait(conn.stream(), nap);
    }
    if let Some(open) = popup.take() {
        conn.ungrab_pointer(x11rb::CURRENT_TIME)?;
        conn.destroy_window(open.id)?;
        conn.flush()?;
    }
    Ok(())
}

/// Sleeps up to `nap`, or less if the X server has something to say.
fn wait(conn: &impl AsRawFd, nap: Duration) {
    let mut poll = libc::pollfd { fd: conn.as_raw_fd(), events: libc::POLLIN, revents: 0 };
    // SAFETY: one valid pollfd, count 1.
    unsafe { libc::poll(&mut poll, 1, nap.as_millis() as i32 + 1) };
}

fn handle(
    conn: &impl Connection,
    event: Event,
    windows: &mut [Window],
    popup: &mut Option<Popup>,
    shared: &Shared,
    text: &mut Text,
    control: &mut Control,
) -> Result<(), Box<dyn Error>> {
    let (screen_w, screen_h) = shared.screen;
    let find = |windows: &[Window], id: u32| windows.iter().position(|window| window.id == id);

    // The open menu has the pointer grabbed: every pointer event is for it.
    if let Some(open) = popup.as_mut() {
        let (w, h) = menu::size(open.items.len());
        let inside = |x: i16, y: i16| x >= 0 && y >= 0 && x < w as i16 && y < h as i16;
        match event {
            Event::MotionNotify(motion) => {
                let hover = inside(motion.event_x, motion.event_y).then(|| menu::hit(open.items.len(), f32::from(motion.event_x), f32::from(motion.event_y))).flatten();
                if hover != open.hover {
                    open.hover = hover;
                    present(conn, shared.depth, open.id, shared.gc, &menu::draw(&open.items, hover, text))?;
                }
                return Ok(());
            }
            Event::ButtonPress(press) => {
                let chosen = inside(press.event_x, press.event_y)
                    .then(|| menu::hit(open.items.len(), f32::from(press.event_x), f32::from(press.event_y)))
                    .flatten()
                    .map(|item| (open.items[item].0, open.widget));
                close_popup(conn, popup)?;
                if let Some((choice, id)) = chosen {
                    interact::choose(choice, id);
                }
                return Ok(());
            }
            Event::ButtonRelease(_) => return Ok(()),
            Event::Expose(expose) if expose.window == open.id => {
                present(conn, shared.depth, open.id, shared.gc, &menu::draw(&open.items, open.hover, text))?;
                return Ok(());
            }
            _ => {}
        }
    }

    match event {
        // Whenever the window manager has really put a window on screen (or any part of it was
        // uncovered), what it shows is sent again: what was drawn before it was mapped is lost.
        Event::Expose(ExposeEvent { window, .. }) | Event::MapNotify(MapNotifyEvent { window, .. }) | Event::VisibilityNotify(VisibilityNotifyEvent { window, .. }) => {
            if let Some(index) = find(windows, window) {
                if let Some(pixmap) = &windows[index].pixmap {
                    present(conn, shared.depth, window, shared.gc, pixmap)?;
                }
            }
        }
        Event::ButtonPress(press) => {
            let Some(index) = find(windows, press.event) else { return Ok(()) };
            let window = &mut windows[index];
            if press.detail == 1 && window.placed.drag {
                window.drag = Some(Drag { root: (press.root_x, press.root_y), from: window.at, moved: false });
            } else if press.detail == 3 && window.placed.menu {
                *popup = Some(open_popup(conn, shared, window.placed.id, window.placed.drag, (press.root_x, press.root_y), text)?);
            }
        }
        Event::MotionNotify(motion) => {
            let Some(index) = find(windows, motion.event) else { return Ok(()) };
            let window = &mut windows[index];
            if let Some(drag) = &mut window.drag {
                let (dx, dy) = (motion.root_x - drag.root.0, motion.root_y - drag.root.1);
                if !drag.moved && dx.abs() < DRAG_SLOP && dy.abs() < DRAG_SLOP {
                    return Ok(());
                }
                drag.moved = true;
                let x = (drag.from.0 + dx).clamp(0, (screen_w as i16 - window.size.0 as i16).max(0));
                let y = (drag.from.1 + dy).clamp(0, (screen_h as i16 - window.size.1 as i16).max(0));
                window.at = (x, y);
                conn.configure_window(window.id, &ConfigureWindowAux::new().x(i32::from(x)).y(i32::from(y)))?;
            }
        }
        Event::ButtonRelease(release) => {
            let Some(index) = find(windows, release.event) else { return Ok(()) };
            let window = &mut windows[index];
            if let Some(drag) = window.drag.take() {
                if drag.moved {
                    let (fx, fy) = (f32::from(window.at.0) / f32::from(screen_w), f32::from(window.at.1) / f32::from(screen_h));
                    window.placed.x = fx;
                    window.placed.y = fy;
                    interact::dropped(window.placed.id, fx, fy, control);
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Opens the menu at the pointer (kept on the screen) as an unmanaged window
/// of its own, and takes the pointer so a click elsewhere closes it.
fn open_popup(conn: &impl Connection, shared: &Shared, widget: u32, drag: bool, (px, py): (i16, i16), text: &mut Text) -> Result<Popup, Box<dyn Error>> {
    let items = menu::items(drag);
    let (w, h) = menu::size(items.len());
    let x = px.min((shared.screen.0 as i16 - w as i16).max(0));
    let y = py.min((shared.screen.1 as i16 - h as i16).max(0));
    let id = conn.generate_id()?;
    conn.create_window(
        shared.depth,
        id,
        shared.root,
        x,
        y,
        w as u16,
        h as u16,
        0,
        WindowClass::INPUT_OUTPUT,
        shared.visual,
        &CreateWindowAux::new()
            .border_pixel(0)
            .colormap(shared.colormap)
            .override_redirect(1)
            .event_mask(EventMask::EXPOSURE | EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION),
    )?;
    conn.map_window(id)?;
    conn.configure_window(id, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE))?;
    present(conn, shared.depth, id, shared.gc, &menu::draw(&items, None, text))?;
    conn.grab_pointer(
        false,
        id,
        EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
        GrabMode::ASYNC,
        GrabMode::ASYNC,
        x11rb::NONE,
        x11rb::NONE,
        x11rb::CURRENT_TIME,
    )?;
    Ok(Popup { id, widget, items, hover: None, opened: Instant::now() })
}

fn close_popup(conn: &impl Connection, popup: &mut Option<Popup>) -> Result<(), Box<dyn Error>> {
    if let Some(open) = popup.take() {
        conn.ungrab_pointer(x11rb::CURRENT_TIME)?;
        conn.destroy_window(open.id)?;
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

/// One widget's window: ARGB, dock type, "below" state (see the top of this file), cleared to
/// transparent so no part of it can ever show what was in that memory before.
fn make_window(conn: &impl Connection, screen: &x11rb::protocol::xproto::Screen, depth: u8, visual: u32, colormap: u32, placed: Placed) -> Result<Window, Box<dyn Error>> {
    let (screen_w, screen_h) = (screen.width_in_pixels, screen.height_in_pixels);
    let (w, h) = widgets::size_of(&placed);
    let size = ((w as f32 * placed.scale) as u16, (h as f32 * placed.scale) as u16);
    let at = ((placed.x * f32::from(screen_w)) as i16, (placed.y * f32::from(screen_h)) as i16);
    let id = conn.generate_id()?;
    let mut mask = EventMask::EXPOSURE | EventMask::STRUCTURE_NOTIFY | EventMask::VISIBILITY_CHANGE;
    if placed.interactive() {
        mask |= EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION | EventMask::LEAVE_WINDOW;
    }
    conn.create_window(
        depth,
        id,
        screen.root,
        at.0,
        at.1,
        size.0,
        size.1,
        0,
        WindowClass::INPUT_OUTPUT,
        visual,
        &CreateWindowAux::new().background_pixel(0).border_pixel(0).colormap(colormap).event_mask(mask),
    )?;
    mark_as_widget(conn, id, &placed, at, size)?;
    conn.map_window(id)?;
    Ok(Window { id, placed, at, size, pixmap: None, due: Instant::now(), drag: None })
}

/// Brings the windows in line with the new settings, touching only what differs.
fn apply_change(conn: &impl Connection, screen: &x11rb::protocol::xproto::Screen, depth: u8, visual: u32, colormap: u32, windows: &mut Vec<Window>, new: &Widgets) -> Result<(), Box<dyn Error>> {
    windows.retain(|window| {
        let kept = new.placed.iter().any(|placed| placed.id == window.placed.id);
        if !kept {
            let _ = conn.destroy_window(window.id);
        }
        kept
    });
    let (screen_w, screen_h) = (screen.width_in_pixels, screen.height_in_pixels);
    for placed in &new.placed {
        match windows.iter().position(|window| window.placed.id == placed.id) {
            Some(index) => {
                let old = &windows[index].placed;
                let same = old.kind == placed.kind && old.skin == placed.skin && (old.scale - placed.scale).abs() < 1e-6 && (old.opacity - placed.opacity).abs() < 1e-6 && old.interactive() == placed.interactive();
                if !same {
                    conn.destroy_window(windows[index].id)?;
                    windows[index] = make_window(conn, screen, depth, visual, colormap, placed.clone())?;
                    continue;
                }
                let at = ((placed.x * f32::from(screen_w)) as i16, (placed.y * f32::from(screen_h)) as i16);
                if at != windows[index].at && windows[index].drag.is_none() {
                    conn.configure_window(windows[index].id, &ConfigureWindowAux::new().x(i32::from(at.0)).y(i32::from(at.1)))?;
                    windows[index].at = at;
                }
                windows[index].placed = placed.clone();
            }
            None => windows.push(make_window(conn, screen, depth, visual, colormap, placed.clone())?),
        }
    }
    conn.flush()?;
    Ok(())
}

fn mark_as_widget(conn: &impl Connection, window: u32, placed: &Placed, at: (i16, i16), size: (u16, u16)) -> Result<(), Box<dyn Error>> {
    conn.change_property32(PropMode::REPLACE, window, atom(conn, b"_NET_WM_WINDOW_TYPE")?, AtomEnum::ATOM, &[atom(conn, b"_NET_WM_WINDOW_TYPE_DOCK")?])?;
    // Every workspace, out of the taskbar, Alt+Tab and pagers, and in the bottom layer.
    conn.change_property32(PropMode::REPLACE, window, atom(conn, b"_NET_WM_DESKTOP")?, AtomEnum::CARDINAL, &[0xFFFF_FFFF])?;
    conn.change_property32(
        PropMode::REPLACE,
        window,
        atom(conn, b"_NET_WM_STATE")?,
        AtomEnum::ATOM,
        &[atom(conn, b"_NET_WM_STATE_BELOW")?, atom(conn, b"_NET_WM_STATE_STICKY")?, atom(conn, b"_NET_WM_STATE_SKIP_TASKBAR")?, atom(conn, b"_NET_WM_STATE_SKIP_PAGER")?],
    )?;
    conn.change_property8(PropMode::REPLACE, window, AtomEnum::WM_NAME, AtomEnum::STRING, b"Paperloom widget")?;
    // Where it was put is where it stays: the window manager is told the position and size are the user's.
    let mut hints = x11rb::properties::WmSizeHints::new();
    hints.position = Some((x11rb::properties::WmSizeHintsSpecification::UserSpecified, i32::from(at.0), i32::from(at.1)));
    hints.size = Some((x11rb::properties::WmSizeHintsSpecification::UserSpecified, i32::from(size.0), i32::from(size.1)));
    hints.min_size = Some((i32::from(size.0), i32::from(size.1)));
    hints.max_size = Some((i32::from(size.0), i32::from(size.1)));
    hints.set_normal_hints(conn, window)?;
    // Never takes focus: input hint off, and a user time of 0 so the window manager
    // doesn't focus it when it appears (see the wallpaper engine's window).
    conn.change_property32(PropMode::REPLACE, window, AtomEnum::WM_HINTS, AtomEnum::WM_HINTS, &[1, 0, 0, 0, 0, 0, 0, 0, 0])?;
    conn.change_property32(PropMode::REPLACE, window, atom(conn, b"_NET_WM_USER_TIME")?, AtomEnum::CARDINAL, &[0])?;
    if !placed.interactive() {
        // An empty input shape: every click goes to what is underneath.
        conn.shape_rectangles(shape::SO::SET, SK::INPUT, x11rb::protocol::xproto::ClipOrdering::UNSORTED, window, 0, 0, &[])?;
    }
    Ok(())
}

fn present(conn: &impl Connection, depth: u8, window: u32, gc: u32, pixmap: &Pixmap) -> Result<(), Box<dyn Error>> {
    let mut bytes = pixmap.data().to_vec();
    for pixel in bytes.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    conn.put_image(ImageFormat::Z_PIXMAP, window, gc, pixmap.width() as u16, pixmap.height() as u16, 0, 0, 0, depth, &bytes)?;
    Ok(())
}
