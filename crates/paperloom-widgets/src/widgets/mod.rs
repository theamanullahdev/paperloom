//! The built-in widgets. Each one turns "now" into a pixmap and says how soon
//! it needs to be drawn again, so nothing runs more often than it must.

pub mod clock;

use std::time::Duration;

use tiny_skia::Pixmap;

use crate::render::Text;

pub struct Frame {
    pub pixmap: Pixmap,
    /// How long until what is shown changes.
    pub next_change: Duration,
}

/// Natural size of a widget in pixels at scale 1.0.
pub fn size_of(_kind: &str) -> (u32, u32) {
    (280, 120)
}

pub fn render(kind: &str, scale: f32, text: &mut Text) -> Option<Frame> {
    match kind {
        "clock" => Some(clock::render(scale, text)),
        _ => None,
    }
}
