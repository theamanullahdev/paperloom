//! Where a widget's pixels go: one transparent window per widget, placed
//! above the wallpaper, below the desktop icons and below every normal
//! window. One backend per platform.

#[cfg(all(unix, not(target_os = "macos")))]
pub mod wayland;
#[cfg(all(unix, not(target_os = "macos")))]
pub mod x11;

/// The Wayland backend when the session is Wayland (the same test the engine
/// uses), otherwise X11.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        wayland::run()
    } else {
        x11::run()
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
