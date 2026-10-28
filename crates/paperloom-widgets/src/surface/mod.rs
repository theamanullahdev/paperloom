//! Where a widget's pixels go: one transparent window per widget, placed
//! above the wallpaper, below the desktop icons and below every normal
//! window. One backend per platform.

#[cfg(all(unix, not(target_os = "macos")))]
pub mod x11;

#[cfg(all(unix, not(target_os = "macos")))]
pub use x11::run;
