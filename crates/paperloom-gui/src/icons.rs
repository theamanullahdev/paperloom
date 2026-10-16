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
