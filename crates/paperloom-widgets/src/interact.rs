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
    if let Some(target) = all.placed.iter_mut().find(|placed| placed.id == id) {
        target.x = x.clamp(0.0, 0.95);
        target.y = y.clamp(0.0, 0.95);
        if all.save().is_ok() {
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
