//! Types and persistence shared between `paperloom-gui` and `paperloom-engine`.
//!
//! Kept dependency-light on purpose: `paperloom-engine` is the always-on
//! piece and must not drag in anything heavy (no GUI toolkit deps here).

pub mod autostart;
mod config_store;
pub mod dev;
pub mod diagnostics;
mod display;
pub mod hotkey;
/// GUI <-> engine wire protocol. Kept as its own namespace (`ipc::Command`,
/// not re-exported flat) since `Command` would otherwise collide with
/// `std::process::Command`, which the GUI also needs for spawning the
/// engine.
pub mod ipc;
mod library;
pub mod lock;
pub mod log;
pub mod media;
pub mod os_auth;
mod paths;
pub mod process;
pub mod usage_history;
pub mod rotation;
pub mod battery;
pub mod stacks;
pub mod widgets;
mod settings;
mod state;

pub use config_store::{load, save};
pub use display::{primary_aspect_ratio, primary_size, work_area};
pub use library::{fingerprint, Collection, Library, LibraryEntry, Optimize, Origin};
pub use paths::rotation_file as paths_rotation_file;
pub use paths::usage_history_file;
pub use paths::skins_dir;
pub use paths::stacks_dir;
pub use paths::widgets_lock;
pub use paths::{background_frames_dir, config_file, dev_file, logs_dir, downloads_dir, engine_log, library_dir, online_cache_dir, state_file, thumbnails_dir, updates_dir, wallpapers_dir};
pub use state::{EngineInfo, EngineState, SavedSetting};
pub use settings::{pick_monitor, ImportMode, ScreenTarget, LibraryLayout, LibraryView, Settings, SourceSetting, ThemeMode, TraySize, TrayStyle, UpdateSettings, ViewSize, SOURCE_MIXKIT, SOURCE_MOTIONBGS, SOURCE_PEXELS, SOURCE_PIXABAY, SOURCE_WALLPAPERWAVES};
