//! Types and persistence shared between `paperloom-gui` and `paperloom-engine`.
//!
//! Kept dependency-light on purpose: `paperloom-engine` is the always-on
//! piece and must not drag in anything heavy (no GUI toolkit deps here).

mod config_store;
/// GUI <-> engine wire protocol. Kept as its own namespace (`ipc::Command`,
/// not re-exported flat) since `Command` would otherwise collide with
/// `std::process::Command`, which the GUI also needs for spawning the
/// engine.
pub mod ipc;
mod library;
mod paths;
mod settings;

pub use config_store::{load, save};
pub use library::{Library, LibraryEntry};
pub use paths::{config_file, library_dir, thumbnails_dir};
pub use settings::{ImportMode, Settings, ThemeMode};
