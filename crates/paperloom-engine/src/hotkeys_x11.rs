//! Global shortcuts on X11: grabs the configured keys on the root window and
//! turns their presses into `player::Shortcut`s. Re-reads the two settings
//! on the engine's tick and re-grabs only when they changed.

use paperloom_common::hotkey::Hotkey;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt as _, GrabMode, ModMask, Window};
use x11rb::protocol::Event;

use crate::player::Shortcut;

pub struct Hotkeys {
    root: Window,
    wanted: (String, String),
    /// (keycode, modifiers, what it does) for every grab made.
    grabbed: Vec<(u8, u16, u8)>,
}

impl Hotkeys {
    pub fn new(root: Window) -> Self {
        Self { root, wanted: (String::new(), String::new()), grabbed: Vec::new() }
    }

    pub fn sync(&mut self, conn: &impl Connection, next: &str, pause: &str) {
        if self.wanted.0 == next && self.wanted.1 == pause {
            return;
        }
        self.wanted = (next.to_string(), pause.to_string());
        for (code, modifiers, _) in self.grabbed.drain(..) {
            let _ = conn.ungrab_key(code, self.root, ModMask::from(modifiers));
        }
        for (index, text) in [next, pause].into_iter().enumerate() {
            let Some(hotkey) = Hotkey::parse(text) else { continue };
            let Some(code) = keycode_for(conn, hotkey.x11_keysym()) else { continue };
            // Also grab with Caps Lock / Num Lock on, or the shortcut dies when they are.
            for lock in [0u16, 2, 16, 18] {
                let modifiers = hotkey.x11_modifiers() | lock;
                if conn.grab_key(false, self.root, ModMask::from(modifiers), code, GrabMode::ASYNC, GrabMode::ASYNC).is_ok() {
                    self.grabbed.push((code, modifiers, index as u8));
                }
            }
        }
        let _ = conn.flush();
    }

    pub fn event(&self, event: &Event) -> Option<Shortcut> {
        let Event::KeyPress(press) = event else { return None };
        let state = u16::from(press.state) & !(2 | 16);
        self.grabbed
            .iter()
            .find(|(code, modifiers, _)| *code == press.detail && (*modifiers & !(2 | 16)) == state)
            .map(|(_, _, which)| if *which == 0 { Shortcut::Next } else { Shortcut::TogglePause })
    }
}

fn keycode_for(conn: &impl Connection, keysym: u32) -> Option<u8> {
    let setup = conn.setup();
    let (first, last) = (setup.min_keycode, setup.max_keycode);
    let mapping = conn.get_keyboard_mapping(first, last - first + 1).ok()?.reply().ok()?;
    let per = usize::from(mapping.keysyms_per_keycode);
    mapping.keysyms.chunks(per).position(|symbols| symbols.contains(&keysym)).map(|index| first + index as u8)
}
