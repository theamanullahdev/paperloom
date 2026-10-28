//! `paperloom-widgets` — draws the desktop widgets. A separate process from
//! the wallpaper engine: it never touches the wallpaper, and the wallpaper
//! never draws a widget, so neither can slow the other down.

mod render;
mod surface;
mod widgets;

use std::process::ExitCode;

fn main() -> ExitCode {
    paperloom_common::log::init("widgets");
    let result = surface::run();
    if let Err(err) = result {
        eprintln!("paperloom-widgets: {err}");
        paperloom_common::log::error(&format!("widgets stopped: {err}"));
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
