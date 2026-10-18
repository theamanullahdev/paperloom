//! Online wallpaper sources and a resumable download queue.
//!
//! Kept separate from `paperloom-common` so the always-on `paperloom-engine`
//! never links an HTTP/TLS stack — only the GUI depends on this crate.

mod aggregate;
mod download;
mod generic;
mod http;
mod motionbgs;
mod net;
mod pexels;
mod pixabay;
mod queue;
mod source;
mod thumbs;
pub mod update;

pub use http::FetchError;
pub use net::is_online;
pub use queue::{Job, Queue, Status};
pub use aggregate::{fetch_page, Cursor, Page};
pub use source::{Listing, OnlineItem, Variant};
pub use thumbs::fetch as fetch_thumbnail;
