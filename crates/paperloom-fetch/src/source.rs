//! Online wallpaper sources. Adding a site means implementing `Source`
//! (parse its listing pages into `OnlineItem`s with a direct download URL)
//! and adding it to `all()` — the download queue and GUI don't care where
//! an item came from.

use crate::http::FetchError;

#[derive(Debug, Clone, PartialEq)]
pub struct OnlineItem {
    /// Stable per-source identifier, used to avoid queueing the same
    /// wallpaper twice.
    pub id: String,
    pub source: &'static str,
    pub title: String,
    /// Short quality label as the site shows it ("4K", "HD", ...).
    pub quality: String,
    pub thumb_url: String,
    /// Direct URL of the MP4 (no further scraping needed to download it).
    pub download_url: String,
    /// Human-facing page, for attribution/"open in browser".
    pub page_url: String,
}

#[derive(Debug, Clone, Default)]
pub struct Listing {
    pub items: Vec<OnlineItem>,
    /// Pass back as `base` to fetch following pages (it's the URL the
    /// site settled on after redirects, e.g. a search that turns into a
    /// tag page).
    pub base: String,
    pub has_more: bool,
}

pub trait Source: Send + Sync {
    fn name(&self) -> &'static str;

    /// Fetches page `page` (1-based). `query` empty means "latest".
    /// `base` is the `Listing::base` from page 1 of the same query.
    fn fetch(&self, query: &str, page: u32, base: Option<&str>) -> Result<Listing, FetchError>;
}

pub fn all() -> Vec<Box<dyn Source>> {
    vec![Box::new(crate::motionbgs::MotionBgs)]
}
