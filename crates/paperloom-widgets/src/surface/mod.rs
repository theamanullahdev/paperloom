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
