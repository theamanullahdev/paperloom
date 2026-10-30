//! Where a widget's pixels go: one transparent window per widget, placed
//! above the wallpaper, below the desktop icons and below every normal
//! window. One backend per platform. Each `run` draws until `control.changed()` says
//! the settings moved, then returns (its windows go with it).

#[cfg(all(unix, not(target_os = "macos")))]
use {crate::control::Control, crate::render::Text, paperloom_common::widgets::Widgets};

#[cfg(all(unix, not(target_os = "macos")))]
pub mod wayland;
#[cfg(all(unix, not(target_os = "macos")))]
pub mod x11;

/// The Wayland backend when the session is Wayland (the same test the engine
/// uses), otherwise X11.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn run(widgets: &Widgets, text: &mut Text, control: &mut Control) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        wayland::run(widgets, text, control)
    } else {
        x11::run(widgets, text, control)
    }
}

#[cfg(windows)]
pub mod windows;

#[cfg(windows)]
pub use windows::run;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "macos")]
pub use macos::run;

/// What `--test` shows: a clock, the date and two meters, whatever the user has placed.
pub fn test_widgets() -> paperloom_common::widgets::Widgets {
    let mut widgets = paperloom_common::widgets::Widgets { enabled: true, placed: Vec::new(), ..Default::default() };
    for (kind, x, y) in [("clock", 0.08, 0.12), ("date", 0.08, 0.5), ("cpu", 0.42, 0.12), ("memory", 0.42, 0.5)] {
        if let Some(id) = widgets.add(kind) {
            if let Some(placed) = widgets.placed.iter_mut().find(|placed| placed.id == id) {
                placed.x = x;
                placed.y = y;
            }
        }
    }
    widgets
}

/// `--probe` on platforms where the screen can't be read back: nothing to report.
#[cfg(not(windows))]
pub fn probe() -> Vec<String> {
    vec!["[INFO] the on-screen check is available on Windows only".to_string()]
}

#[cfg(windows)]
pub use windows::{probe, show_desktop};

/// `--show-desktop on|off`: minimises other windows so the desktop can be looked at (and brings them back).
#[cfg(not(windows))]
pub fn show_desktop(_on: bool) {}
