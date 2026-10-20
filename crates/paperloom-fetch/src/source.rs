//! Online wallpaper sources. A source turns (query, page) into a list of
//! `OnlineItem`s with direct download URLs; the aggregator, download queue
//! and GUI don't care where an item came from. Built-ins (MotionBGS,
//! Pexels, Pixabay) are chosen by `SourceSetting::id`; anything else is a
//! user-added page source.

use paperloom_common::{SourceSetting, SOURCE_MIXKIT, SOURCE_MOTIONBGS, SOURCE_PEXELS, SOURCE_PIXABAY, SOURCE_WALLPAPERWAVES};

use crate::http::FetchError;

/// One downloadable rendition of a wallpaper.
#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    /// "HD" or "4K".
    pub label: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OnlineItem {
    /// Source-native identifier.
    pub id: String,
    /// `SourceSetting::id` of the source it came from.
    pub source: String,
    pub source_name: String,
    pub title: String,
    pub thumb_url: String,
    /// Always at least one; HD first when both exist.
    pub variants: Vec<Variant>,
    /// Human-facing page, for attribution.
    pub page_url: String,
}

impl OnlineItem {
    /// Globally unique key (also the download job id and the library's
    /// `source_id`).
    pub fn key(&self) -> String {
        format!("{}:{}", self.source, self.id)
    }

    pub fn variant(&self, label: &str) -> Option<&Variant> {
        self.variants.iter().find(|variant| variant.label == label)
    }

    pub fn has_4k(&self) -> bool {
        self.variant("4K").is_some()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Listing {
    pub items: Vec<OnlineItem>,
    /// Pass back as `base` to fetch following pages (a source may settle on
    /// a different URL after redirects, e.g. a search turning into a tag
    /// page).
    pub base: String,
    pub has_more: bool,
}

pub trait Source: Send + Sync {
    /// Fetches page `page` (1-based). `query` empty means "latest".
    /// `base` is the `Listing::base` from page 1 of the same query.
    fn fetch(&self, query: &str, page: u32, base: Option<&str>) -> Result<Listing, FetchError>;
}

/// `safe_search`: ask sources that support it to leave out adult results.
pub fn build(setting: &SourceSetting, safe_search: bool) -> Box<dyn Source> {
    match setting.id.as_str() {
        SOURCE_MOTIONBGS => Box::new(crate::motionbgs::MotionBgs),
        SOURCE_PEXELS => Box::new(crate::pexels::Pexels { key: setting.api_key.clone() }),
        SOURCE_PIXABAY => Box::new(crate::pixabay::Pixabay { key: setting.api_key.clone(), safe_search }),
        SOURCE_MIXKIT => Box::new(crate::mixkit::Mixkit),
        SOURCE_WALLPAPERWAVES => Box::new(crate::wallpaperwaves::WallpaperWaves),
        // Only sites the user added come through the general reader.
        _ => Box::new(crate::generic::PageSource {
            id: setting.id.clone(),
            name: setting.name.clone(),
            url: setting.url.clone(),
            key: setting.api_key.clone(),
        }),
    }
}

/// A download URL may be a wallpaper's *page* rather than its video (the
/// server told us so by sending HTML). Each known site finds the real file
/// its own way; anything else falls back to the best `.mp4` link on the
/// page.
pub fn resolve_page(source_id: &str, page_url: &str, body: &str) -> Result<String, FetchError> {
    match source_id {
        SOURCE_WALLPAPERWAVES => crate::wallpaperwaves::resolve_download(body),
        _ => crate::generic::best_mp4(body, page_url),
    }
}
