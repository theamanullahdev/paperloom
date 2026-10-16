//! Online wallpaper sources and a resumable download queue.
//!
//! Kept separate from `paperloom-common` so the always-on `paperloom-engine`
//! never links an HTTP/TLS stack — only the GUI depends on this crate.

mod download;
mod http;
mod motionbgs;
mod queue;
mod source;
mod thumbs;

pub use http::FetchError;
pub use queue::{Job, Queue, Status};
pub use source::{all as sources, Listing, OnlineItem, Source};
pub use thumbs::fetch as fetch_thumbnail;
