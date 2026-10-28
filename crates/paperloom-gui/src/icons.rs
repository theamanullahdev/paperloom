//! Icons (`assets/icons/`), vendored from Lucide (ISC-licensed —
//! `assets/icons/LICENSE-lucide.txt`) rather than hand-drawn: a real,
//! consistently-designed icon set beats freehanding SVG paths from
//! scratch. Library nav uses `clapperboard.svg` (distinct from a plain
//! grid/gear), Settings uses `sliders-horizontal.svg`.
//!
//! Iced's `svg` widget recolors at render time via
//! `Style { color: Some(..) }`, so the same file works for every
//! theme/state without separate colored copies.

use iced::widget::svg;

pub fn library() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/library.svg").as_slice())
}

pub fn settings() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/settings.svg").as_slice())
}

/// Placeholder tile for an entry whose thumbnail failed to generate (no
/// `ffmpeg` on `PATH`, or extraction failed).
pub fn film() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/film.svg").as_slice())
}

/// The top-right kebab menu on a gallery card (secondary actions like
/// "Remove", kept out of the card's primary bottom button bar).
pub fn ellipsis() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/ellipsis.svg").as_slice())
}

/// Nav icon for the online library.
pub fn globe() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/globe.svg").as_slice())
}

pub fn download() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/download.svg").as_slice())
}

/// Drag handle on a download row.
pub fn grip() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/grip-vertical.svg").as_slice())
}

pub fn move_to_top() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/arrow-up-to-line.svg").as_slice())
}

pub fn wifi_off() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/wifi-off.svg").as_slice())
}

pub fn grid() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/layout-grid.svg").as_slice())
}

pub fn list() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/list.svg").as_slice())
}

/// "Set as wallpaper" on compact cards and rows.
pub fn play() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/play.svg").as_slice())
}

pub fn back() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/chevron-left.svg").as_slice())
}

/// An empty collection's cover.
pub fn folder() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/folder.svg").as_slice())
}

/// Collapsed group in the sidebar tree.
pub fn chevron_right() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/chevron-right.svg").as_slice())
}

/// Expanded group in the sidebar tree.
pub fn chevron_down() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/chevron-down.svg").as_slice())
}

pub fn eye_off() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/eye-off.svg").as_slice())
}

pub fn compass() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/compass.svg").as_slice())
}

pub fn layers() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/layers.svg").as_slice())
}

pub fn dashboard() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/layout-dashboard.svg").as_slice())
}

pub fn activity() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/activity.svg").as_slice())
}

pub fn refresh() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/refresh-cw.svg").as_slice())
}

pub fn terminal() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/terminal.svg").as_slice())
}

pub fn import() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/import.svg").as_slice())
}

pub fn share() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/share-2.svg").as_slice())
}

pub fn select() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/square-check.svg").as_slice())
}

/// Widgets.
pub fn clock() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/clock.svg").as_slice())
}
