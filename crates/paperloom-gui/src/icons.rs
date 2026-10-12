//! Hand-drawn sidebar nav icons (`assets/icons/`) — not a generic icon
//! font/library pull. Library: a framed play-triangle (distinct from a
//! plain grid). Settings: three sliders at different positions, not the
//! overused gear glyph. Both drawn at a consistent ~1.75 stroke weight
//! with round caps/joins, 24x24 viewBox — matching the "consistent stroke
//! weight, clear silhouette" principle from Apple's icon guidelines and
//! elementary OS's own HIG (icons should have a distinctive shape, not
//! just a rounded rectangle).
//!
//! Drawn black in the source files; Iced's `svg` widget recolors them at
//! render time via `Style { color: Some(..) }`, so the same file works
//! for every theme/state without separate colored copies.

use iced::widget::svg;

pub fn library() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/library.svg").as_slice())
}

pub fn settings() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!("../assets/icons/settings.svg").as_slice())
}
