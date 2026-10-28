//! `paperloom-widgets` — draws the desktop widgets. A separate process from
//! the wallpaper engine: it never touches the wallpaper, and the wallpaper
//! never draws a widget, so neither can slow the other down.

mod render;
mod surface;
mod widgets;

use std::process::ExitCode;

fn main() -> ExitCode {
    paperloom_common::log::init("widgets");
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = surface::run();
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    let result: Result<(), Box<dyn std::error::Error>> = Err("widgets are not available on this platform in this build".into());
    if let Err(err) = result {
        eprintln!("paperloom-widgets: {err}");
        paperloom_common::log::error(&format!("widgets stopped: {err}"));
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
