#![cfg_attr(windows, windows_subsystem = "windows")]

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
mod selftest;
mod sample;
mod surface;
mod widgets;

use std::process::ExitCode;

use paperloom_common::widgets::{Running, Widgets};

fn main() -> ExitCode {
    paperloom_common::log::init("widgets");
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--selftest") {
        return selftest::run();
    }
    let testing = args.iter().any(|arg| arg == "--test");
    let Some(running) = Running::claim() else {
        if testing {
            println!("The widgets are already running: stop them first (Developers → Widgets → Quit).");
        }
        return ExitCode::SUCCESS;
    };
    let mut text = render::Text::new();
    let mut control = control::Control::new();
    if testing {
        control.test_for(args.iter().position(|arg| arg == "--test").and_then(|at| args.get(at + 1)).and_then(|value| value.parse().ok()).unwrap_or(20));
    }
    paperloom_common::widgets::Status::update(|status| {
        status.problem.clear();
        status.paused.clear();
        status.windows = 0;
        status.backend.clear();
    });
    paperloom_skin::set_user_dir(&paperloom_common::skins_dir());

    loop {
        // A look may have been installed (or removed) since the last pass.
        paperloom_skin::refresh();
        let widgets = if testing { surface::test_widgets() } else { Widgets::load() };
        if !testing && (!widgets.enabled || widgets.placed.is_empty()) {
            return ExitCode::SUCCESS;
        }
        if control.test_over() {
            // The test is over: the user's own widgets come back.
            drop(running);
            let widgets = Widgets::load();
            if widgets.enabled && !widgets.placed.is_empty() {
                let _ = paperloom_common::widgets::start();
            }
            return ExitCode::SUCCESS;
        }
        control.mark_seen();
        widgets::forget();
        control.sample.set_needs(widgets.placed.iter().map(|placed| placed.kind.as_str()));
        if let Err(err) = surface::run(&widgets, &mut text, &mut control) {
            eprintln!("paperloom-widgets: {err}");
            paperloom_common::log::error(&format!("widgets stopped: {err}"));
            paperloom_common::widgets::Status::update(|status| status.problem = err.to_string());
            return ExitCode::FAILURE;
        }
    }
}
