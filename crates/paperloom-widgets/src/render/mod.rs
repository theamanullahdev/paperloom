//! Drawing: a pixmap per widget, text through cosmic-text (shaping, so
//! right-to-left and complex scripts render correctly) and tiny-skia shapes.

pub mod card;
pub mod text;

pub use text::Text;

/// Paperloom's dark theme colours (see DESIGN.md), as RGBA bytes.
pub mod palette {
    pub const CARD: [u8; 4] = [0x26, 0x22, 0x20, 0xD0];
    pub const BORDER: [u8; 4] = [0xFF, 0xFF, 0xFF, 0x14];
    pub const FOREGROUND: [u8; 4] = [0xF5, 0xEF, 0xE9, 0xFF];
    pub const MUTED: [u8; 4] = [0xA8, 0x99, 0x8E, 0xFF];
}
