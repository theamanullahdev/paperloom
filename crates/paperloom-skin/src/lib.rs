//! Widget looks ("skins"). A widget is some data (the time, the processor load,
//! …) plus a look: a small text file that lists shapes, text, bars, rings and
//! clock hands, and which of the data goes where. The same files and the same
//! renderer draw the widgets on the desktop and the previews in the app, so
//! what you pick is exactly what you get.
//!
//! No code runs from a skin: it can only describe drawing. That is what makes
//! it safe to install one from the online catalogue.

mod color;
mod draw;
mod element;
mod registry;
mod text;
mod vars;
pub mod rainmeter;

pub use element::Skin;
pub use registry::{all, default_for, for_kind, get, refresh, set_user_dir};
pub use text::{Style, Text};
pub use vars::{clock, meter, preview, Value, Vars};

/// A skin with its draw instructions rendered at `scale`.
pub use draw::render;
