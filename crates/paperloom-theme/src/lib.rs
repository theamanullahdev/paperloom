//! Paperloom's app themes.
//!
//! A theme is a folder with a `theme.toml` (plus optional fonts and
//! pictures). The file is plain data: colours for the light and the dark
//! mode, fonts, corner shapes, where the navigation sits, a backdrop,
//! picture layers, and overrides for named parts of the window. Whatever a
//! theme leaves out comes from the built-in default, so a one-line theme is
//! a valid theme.
//!
//! No toolkit types here (like `paperloom-skin`): the app turns a `Theme`
//! into what it draws with; the MCP server only reads, checks and writes
//! theme files.

mod check;
mod color;
mod model;
mod registry;
mod schema;

pub use check::{check, Report};
pub use color::{contrast, parse as parse_color, to_hex, Rgba};
pub use model::{Anchor, Backdrop, ColorModes, Colors, Fonts, Layer, Layout, Nav, NavStyle, Palette, Part, Shape, Theme, PARTS};
pub use registry::{builtin, find, folder_name, installed, list, remove, save, Listed, Origin, DEFAULT_ID};
pub use schema::SCHEMA;
