//! Wallpaper Waves. Result pages list wallpapers as `<article class="jeg_post
//! moewalls-card">` cards (page link + thumbnail); the real download link
//! (`download.php?video=…`) only appears on each wallpaper's own page, so a
//! download starts from the page and `resolve_download` finds the link then.

use crate::generic::decode_entities;
use crate::http::{self, FetchError};
use crate::motionbgs::url_encode;
use crate::source::{Listing, OnlineItem, Source, Variant};
use paperloom_common::SOURCE_WALLPAPERWAVES;

const SITE: &str = "https://wallpaperwaves.com";

pub struct WallpaperWaves;

impl Source for WallpaperWaves {
    fn fetch(&self, query: &str, page: u32, _base: Option<&str>) -> Result<Listing, FetchError> {
        let url = page_url(query, page);
        let (html, _) = http::get_text(&url)?;
        let has_more = html.contains(&format!("/page/{}/", page + 1));
        Ok(Listing { items: parse_listing(&html), base: String::new(), has_more })
    }
}

fn page_url(query: &str, page: u32) -> String {
    let query = query.trim();
    match (query.is_empty(), page <= 1) {
        (true, true) => format!("{SITE}/"),
        (true, false) => format!("{SITE}/page/{page}/"),
        (false, true) => format!("{SITE}/?s={}", url_encode(query)),
        (false, false) => format!("{SITE}/page/{page}/?s={}", url_encode(query)),
    }
}

pub(crate) fn parse_listing(html: &str) -> Vec<OnlineItem> {
    html.split("<article class=\"jeg_post")
        .skip(1)
        .filter_map(|chunk| {
            let card = chunk.split("</article>").next()?;
            let page_url = text_between(card, "<a href=\"", "\"")?.to_string();
            if !page_url.starts_with(SITE) {
                return None;
            }
            let img = &card[card.find("<img ")?..];
            let img = &img[..img.find('>')?];
            let thumb = best_thumb(img).or_else(|| attr(img, "src"))?;

            // The page's last path part is the wallpaper's slug, e.g.
            // `fallen-knight-in-flower-field-rain-live-wallpaper`.
            let slug = page_url.trim_end_matches('/').rsplit('/').next()?.to_string();
            Some(OnlineItem {
                id: slug.clone(),
                source: SOURCE_WALLPAPERWAVES.to_string(),
                source_name: "Wallpaper Waves".to_string(),
                title: title_from_slug(&slug),
                thumb_url: thumb,
                // Resolved to the real file when the download starts.
                variants: vec![Variant { label: "HD".into(), url: page_url.clone() }],
                page_url,
            })
        })
        .collect()
}

/// The wallpaper's actual video, from its page: the "Download" button's
/// `download.php?video=…` link.
pub(crate) fn resolve_download(page_html: &str) -> Result<String, FetchError> {
    let start = page_html
        .find("download.php?video=")
        .ok_or_else(|| FetchError::Permanent("no download link found on the wallpaper's page".into()))?;
    let link_start = page_html[..start].rfind(['"', '\'']).map_or(start, |quote| quote + 1);
    let rest = &page_html[link_start..];
    let end = rest.find(['"', '\'', ' ', '<']).unwrap_or(rest.len());
    let link = decode_entities(&rest[..end]);
    Ok(if link.starts_with("http") { link } else { format!("{SITE}/{}", link.trim_start_matches('/')) })
}

/// A mid-size entry from the image's `srcset` (the full-size one is a
/// 1920px WebP — far too big for a gallery tile).
fn best_thumb(img: &str) -> Option<String> {
    let srcset = attr(img, "srcset")?;
    let mut best: Option<(u32, String)> = None;
    for entry in srcset.split(',') {
        let mut parts = entry.split_whitespace();
        let (Some(url), Some(width)) = (parts.next(), parts.next()) else { continue };
        let Ok(width) = width.trim_end_matches('w').parse::<u32>() else { continue };
        // Closest to 600px wide that is at least 364.
        if width >= 364 && best.as_ref().is_none_or(|(current, _)| width.abs_diff(600) < current.abs_diff(600)) {
            best = Some((width, url.to_string()));
        }
    }
    best.map(|(_, url)| url)
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let needle = format!(" {name}=\"");
    let from = tag.find(&needle)? + needle.len();
    let rest = &tag[from..];
    Some(rest[..rest.find('"')?].to_string())
}

fn text_between<'a>(haystack: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let from = haystack.find(start)? + start.len();
    let rest = &haystack[from..];
    Some(&rest[..rest.find(end)?])
}

fn title_from_slug(slug: &str) -> String {
    let words: Vec<&str> = slug.split('-').filter(|word| !word.is_empty()).collect();
    // Drop the "live wallpaper" suffix every slug carries.
    let words = match words.as_slice() {
        [head @ .., "live", "wallpaper"] => head.to_vec(),
        _ => words,
    };
    let title = words
        .iter()
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map(|first| first.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ");
    if title.is_empty() { "Wallpaper".to_string() } else { title }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"<article class="jeg_post moewalls-card format-standard"><div class="jeg_thumb"><a href="https://wallpaperwaves.com/fantasy/fallen-knight-in-flower-field-rain-live-wallpaper/">
<img width="1920" height="1080" src="https://wallpaperwaves.com/u/full.webp" class="x" alt="A long description" srcset="https://wallpaperwaves.com/u/a-300x169.webp 300w, https://wallpaperwaves.com/u/a-768x432.webp 768w, https://wallpaperwaves.com/u/a.webp 1920w" title="Fallen Knight 1"> </a></div></article>
<article class="jeg_post moewalls-card"><a href="https://elsewhere.test/x/"><img src="s"></a></article>"#;

    #[test]
    fn parses_cards_with_a_sensible_thumbnail_and_clean_title() {
        let items = parse_listing(PAGE);
        assert_eq!(items.len(), 1, "cards linking off-site are skipped");
        let item = &items[0];
        assert_eq!(item.id, "fallen-knight-in-flower-field-rain-live-wallpaper");
        assert_eq!(item.title, "Fallen Knight In Flower Field Rain");
        assert_eq!(item.thumb_url, "https://wallpaperwaves.com/u/a-768x432.webp");
        assert_eq!(item.variant("HD").unwrap().url, item.page_url);
    }

    #[test]
    fn finds_the_download_link_on_a_wallpaper_page() {
        let html = r#"<a class="wp-block-button__link" href="https://wallpaperwaves.com/download.php?video=public_html/26/x-wallpaperwaves-com.mp4" style="x">Download</a>
            <source src="https://wallpaperwaves.com/wp-content/uploads/x-preview.mp4">"#;
        assert_eq!(resolve_download(html).unwrap(), "https://wallpaperwaves.com/download.php?video=public_html/26/x-wallpaperwaves-com.mp4");
        assert!(resolve_download("<html>nothing</html>").is_err());
    }

    #[test]
    fn builds_urls() {
        assert_eq!(page_url("", 1), "https://wallpaperwaves.com/");
        assert_eq!(page_url("", 2), "https://wallpaperwaves.com/page/2/");
        assert_eq!(page_url("rain", 1), "https://wallpaperwaves.com/?s=rain");
        assert_eq!(page_url("neon city", 3), "https://wallpaperwaves.com/page/3/?s=neon+city");
    }
}
