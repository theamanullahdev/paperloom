//! Pause-trigger detection (PROJECT.md §8 / §6): decides whether the
//! wallpaper should be paused right now. Each trigger is independently
//! toggleable in `Settings` — this module just answers "should we be
//! paused?"; the caller is responsible for polling it and toggling mpv's
//! `pause` property on change.
//!
//! Checked at a low frequency (see `backends::x11`), not every event-loop
//! tick: two of the four checks shell out to `gdbus`/`powerprofilesctl`,
//! and none of lock state, battery-saver state, or what's fullscreen
//! change fast enough to need checking faster than every couple of
//! seconds. That keeps this consistent with "near-zero idle cost" even
//! though it isn't literally zero.

use std::error::Error;
use std::process::Command;

use paperloom_common::Settings;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt as _, Window};

pub struct PauseDetector {
    root: Window,
    net_active_window: Atom,
    net_wm_state: Atom,
    net_wm_state_fullscreen: Atom,
    net_client_list_stacking: Atom,
}

impl PauseDetector {
    pub fn new(conn: &impl Connection, root: Window) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            root,
            net_active_window: intern(conn, b"_NET_ACTIVE_WINDOW")?,
            net_wm_state: intern(conn, b"_NET_WM_STATE")?,
            net_wm_state_fullscreen: intern(conn, b"_NET_WM_STATE_FULLSCREEN")?,
            net_client_list_stacking: intern(conn, b"_NET_CLIENT_LIST_STACKING")?,
        })
    }

    pub fn should_pause(
        &self,
        conn: &impl Connection,
        screen_width: u16,
        screen_height: u16,
        our_window: Window,
        settings: &Settings,
    ) -> bool {
        (settings.pause_on_fullscreen_app && self.active_window_is_fullscreen(conn).unwrap_or(false))
            || (settings.pause_on_window_covering_desktop
                && self.a_window_covers_desktop(conn, screen_width, screen_height, our_window).unwrap_or(false))
            || (settings.pause_on_screen_lock && screen_is_locked())
            || (settings.pause_on_battery_saver && battery_saver_active())
    }

    fn active_window_is_fullscreen(&self, conn: &impl Connection) -> Result<bool, Box<dyn Error>> {
        let Some(active) = get_property_u32(conn, self.root, self.net_active_window)? else {
            return Ok(false);
        };
        if active == 0 {
            return Ok(false);
        }
        let states = get_property_u32_list(conn, active, self.net_wm_state)?;
        Ok(states.contains(&self.net_wm_state_fullscreen))
    }

    /// Approximates "something is covering the desktop" as "the topmost
    /// normal window is at least as big as the screen" (i.e. maximized or
    /// bigger) — not pixel-perfect occlusion, but close enough for a pause
    /// heuristic and far simpler than tracking real window regions.
    fn a_window_covers_desktop(
        &self,
        conn: &impl Connection,
        screen_width: u16,
        screen_height: u16,
        our_window: Window,
    ) -> Result<bool, Box<dyn Error>> {
        let stacking = get_property_u32_list(conn, self.root, self.net_client_list_stacking)?;
        // EWMH: bottom-to-top order, so the topmost window is last.
        for &win in stacking.iter().rev() {
            if win == our_window {
                continue;
            }
            if let Ok(geom) = conn.get_geometry(win)?.reply() {
                if geom.width >= screen_width && geom.height >= screen_height {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}

fn intern(conn: &impl Connection, name: &[u8]) -> Result<Atom, Box<dyn Error>> {
    Ok(conn.intern_atom(false, name)?.reply()?.atom)
}

fn get_property_u32(conn: &impl Connection, window: Window, property: Atom) -> Result<Option<u32>, Box<dyn Error>> {
    let reply = conn.get_property(false, window, property, AtomEnum::ANY, 0, 1)?.reply()?;
    Ok(reply.value32().and_then(|mut values| values.next()))
}

fn get_property_u32_list(conn: &impl Connection, window: Window, property: Atom) -> Result<Vec<u32>, Box<dyn Error>> {
    let reply = conn.get_property(false, window, property, AtomEnum::ANY, 0, u32::MAX)?.reply()?;
    Ok(reply.value32().map(|values| values.collect()).unwrap_or_default())
}

/// Cinnamon-specific (this app's primary target) — `cinnamon-screensaver`'s
/// own D-Bus interface, not the generic freedesktop one, since that's what
/// actually locks the screen on this desktop.
///
/// `pub(crate)`: also used directly by `backends::wayland`, which can't use
/// `PauseDetector` as a whole (its fullscreen/window-covering checks are
/// X11-specific, no portable Wayland equivalent exists) but can still use
/// this one and `battery_saver_active` below, since both are
/// windowing-system-independent.
pub(crate) fn screen_is_locked() -> bool {
    Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.cinnamon.ScreenSaver",
            "--object-path",
            "/org/cinnamon/ScreenSaver",
            "--method",
            "org.cinnamon.ScreenSaver.GetActive",
        ])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).contains("true"))
        .unwrap_or(false)
}

pub(crate) fn battery_saver_active() -> bool {
    Command::new("powerprofilesctl")
        .arg("get")
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim() == "power-saver")
        .unwrap_or(false)
}
