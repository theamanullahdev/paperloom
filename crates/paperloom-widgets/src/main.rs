//! `paperloom-widgets` — draws the desktop widgets. A separate process from
//! the wallpaper engine: it never touches the wallpaper, and the wallpaper
//! never draws a widget, so neither can slow the other down.
//!
//! One copy runs at a time (a lock file). It follows `widgets.toml`: a change
//! is picked up within a couple of seconds, and switching widgets off (or
//! removing the last one) ends the process.

mod control;
mod guard;
mod interact;
mod render;
mod sample;
mod surface;
mod widgets;

use std::process::ExitCode;

use paperloom_common::widgets::{Running, Widgets};

fn main() -> ExitCode {
    paperloom_common::log::init("widgets");
    let Some(_running) = Running::claim() else { return ExitCode::SUCCESS };
    let mut text = render::Text::new();
    let mut control = control::Control::new();
    paperloom_skin::set_user_dir(&paperloom_common::skins_dir());

    loop {
        // A look may have been installed (or removed) since the last pass.
        paperloom_skin::refresh();
        let widgets = Widgets::load();
        if !widgets.enabled || widgets.placed.is_empty() {
            return ExitCode::SUCCESS;
        }
        control.mark_seen();
        widgets::forget();
        control.sample.set_needs(widgets.placed.iter().map(|placed| placed.kind.as_str()));
        if let Err(err) = surface::run(&widgets, &mut text, &mut control) {
            eprintln!("paperloom-widgets: {err}");
            paperloom_common::log::error(&format!("widgets stopped: {err}"));
            return ExitCode::FAILURE;
        }
    }
}
