//! `paperloom-widgets` — draws the desktop widgets. A separate process from
//! the wallpaper engine: it never touches the wallpaper, and the wallpaper
//! never draws a widget, so neither can slow the other down.
//!
//! One copy runs at a time (a lock file). It follows `widgets.toml`: a change
//! is picked up within a couple of seconds, and switching widgets off (or
//! removing the last one) ends the process.

mod render;
mod surface;
mod widgets;

use std::process::ExitCode;
use std::time::{Duration, Instant};

use paperloom_common::widgets::{changed_at, Running, Widgets};

/// How often the settings file is looked at.
const CHECK_EVERY: Duration = Duration::from_secs(2);

fn main() -> ExitCode {
    paperloom_common::log::init("widgets");
    let Some(_running) = Running::claim() else { return ExitCode::SUCCESS };
    let mut text = render::Text::new();

    loop {
        let widgets = Widgets::load();
        if !widgets.enabled || widgets.placed.is_empty() {
            return ExitCode::SUCCESS;
        }
        let seen = changed_at();
        let mut last_check = Instant::now();
        // True once widgets.toml is different from what is on screen.
        let mut changed = move || {
            if last_check.elapsed() < CHECK_EVERY {
                return false;
            }
            last_check = Instant::now();
            changed_at() != seen
        };
        if let Err(err) = surface::run(&widgets, &mut text, &mut changed) {
            eprintln!("paperloom-widgets: {err}");
            paperloom_common::log::error(&format!("widgets stopped: {err}"));
            return ExitCode::FAILURE;
        }
    }
}
