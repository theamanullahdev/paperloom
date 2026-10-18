//! Types and persistence shared between `paperloom-gui` and `paperloom-engine`.
//!
//! Kept dependency-light on purpose: `paperloom-engine` is the always-on
//! piece and must not drag in anything heavy (no GUI toolkit deps here).

pub mod autostart;
mod config_store;
mod display;
/// GUI <-> engine wire protocol. Kept as its own namespace (`ipc::Command`,
/// not re-exported flat) since `Command` would otherwise collide with
/// `std::process::Command`, which the GUI also needs for spawning the
/// engine.
pub mod ipc;
mod library;
mod paths;
pub mod process;
mod settings;
mod state;

pub use config_store::{load, save};
pub use display::primary_aspect_ratio;
pub use library::{Library, LibraryEntry};
pub use paths::{config_file, downloads_dir, library_dir, online_cache_dir, state_file, thumbnails_dir, updates_dir};
pub use state::EngineState;
pub use settings::{ImportMode, Settings, SourceSetting, ThemeMode, UpdateSettings, SOURCE_MOTIONBGS, SOURCE_PEXELS, SOURCE_PIXABAY};
