#![cfg_attr(target_os = "macos", allow(dead_code))] // macOS widgets are click-through: no menu, no drag.
//! What dragging and the right-click menu do to the settings. Each change is
//! made on a fresh read of `widgets.toml`, so nothing the app wrote in the
//! meantime is lost.

use paperloom_common::widgets::{self, Widgets};

use crate::control::Control;
use crate::render::menu::Choice;

/// A widget was dropped at (`x`, `y`), as fractions of the screen. Saved without
/// making the process reload itself: the window is already where it should be.
pub fn dropped(id: u32, x: f32, y: f32, control: &mut Control) {
    let mut all = Widgets::load();
    if let Some(stacked) = all.placed.iter().find(|placed| placed.id == id).map(|placed| placed.stack != 0) {
        // A widget in a stack takes its whole stack along.
        all.move_member(id, x, y, paperloom_common::primary_size().unwrap_or((1920, 1080)));
        // Alone, its window is already in place; a stack's other windows still have to follow.
        if all.save().is_ok() && !stacked {
            control.mark_seen();
        }
    }
}

/// Carries out a menu choice. These change the settings file, which the
/// process then reloads, so the widgets on screen follow.
pub fn choose(choice: Choice, id: u32) {
    match choice {
        Choice::Settings => {
            let _ = paperloom_common::process::command("paperloom-gui").args(["--page", "widgets"]).spawn();
        }
        Choice::ToggleLock => {
            let mut all = Widgets::load();
            if let Some(target) = all.placed.iter_mut().find(|placed| placed.id == id) {
                target.drag = !target.drag;
                let _ = all.save();
            }
        }
        Choice::Remove => {
            let mut all = Widgets::load();
            all.remove(id);
            let _ = all.save();
        }
        Choice::HideAll => {
            let _ = widgets::set_enabled(false);
        }
    }
}
