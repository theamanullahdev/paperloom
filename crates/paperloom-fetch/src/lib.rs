//! Online wallpaper sources and a resumable download queue.
//!
//! Kept separate from `paperloom-common` so the always-on `paperloom-engine`
//! never links an HTTP/TLS stack — only the GUI depends on this crate.

mod aggregate;
mod cache;
mod download;
mod generic;
mod http;
pub mod keywords;
mod mixkit;
mod motionbgs;
mod net;
mod pexels;
mod pixabay;
mod queue;
mod stats;
mod source;
mod thumbs;
mod wallpaperwaves;
pub mod update;

pub use http::FetchError;
pub use net::{diagnose, is_online, Diagnosis, HostCheck, Verdict};
pub use stats::received as bytes_received;
pub use queue::{Job, Queue, Status};
pub use aggregate::{fetch_page, Cursor, Page, MATURE_BLOCKED_NOTICE};
pub use source::{Listing, OnlineItem, Variant};
pub use cache::{load as load_listing, save as save_listing, SavedListing};
pub use thumbs::{cached as cached_thumbnail, fetch as fetch_thumbnail};
