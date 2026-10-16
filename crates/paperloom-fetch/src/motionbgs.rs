//! motionbgs.com. Its pages are plain server-rendered HTML with minified,
//! mostly unquoted attributes; each wallpaper tile is
//! `<a title=".." href=/slug> ..<img src=/i/c/364x205/media/<id>/..>..
//! <span class=ttl>Title</span> <span class=frm> 4K </span></a>`, and
//! `/dl/hd/<id>/` serves the MP4 directly. Only public pages that
//! robots.txt allows are requested (never `/ajax/`, `/a2/`, `/admin/`).

use crate::http::{self, FetchError};
use crate::source::{Listing, OnlineItem, Source, Variant};
use paperloom_common::SOURCE_MOTIONBGS;

const SITE: &str = "https://motionbgs.com";

pub struct MotionBgs;

impl Source for MotionBgs {
    fn fetch(&self, query: &str, page: u32, base: Option<&str>) -> Result<Listing, FetchError> {
        let url = page_url(query, page, base);
        let (html, final_url) = http::get_text(&url)?;

        // Page 1 of a search redirects somewhere stable (`/tag:rain/`);
        // later pages hang off that. If it didn't redirect we have no
        // pageable base and stop after page 1.
        let base = if page == 1 { normalize_base(&final_url) } else { base.map(str::to_string) };
        let pageable = base.is_some() || query.trim().is_empty();
        let has_more = pageable && html.contains(&format!("/{}/", page + 1));

        Ok(Listing { items: parse_listing(&html), base: base.unwrap_or_default(), has_more })
    }
}

fn page_url(query: &str, page: u32, base: Option<&str>) -> String {
    match (query.trim(), page, base) {
        ("", 1, _) => format!("{SITE}/"),
        ("", n, _) => format!("{SITE}/{n}/"),
        (q, 1, _) => format!("{SITE}/search?q={}", url_encode(q)),
        (_, n, Some(base)) => format!("{base}{n}/"),
        (q, _, None) => format!("{SITE}/search?q={}", url_encode(q)),
    }
}

/// `https://motionbgs.com/tag:rain/` -> itself; a still-`/search?...` URL
/// can't be paged, so it yields `None`. The bare homepage is paged as
/// `/2/`, `/3/`, which `page_url` handles without a base.
fn normalize_base(final_url: &str) -> Option<String> {
    if final_url.contains("/search?") {
        return None;
    }
    let url = final_url.split('?').next()?;
    let url = if url.ends_with('/') { url.to_string() } else { format!("{url}/") };
    (url != format!("{SITE}/")).then_some(url)
}

pub(crate) fn url_encode(input: &str) -> String {
    let mut out = String::new();
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

pub(crate) fn parse_listing(html: &str) -> Vec<OnlineItem> {
    let mut items = Vec::new();
    for chunk in html.split("<a title=").skip(1) {
        let Some(tile) = chunk.split("</a>").next() else { continue };
        let Some(title) = text_between(tile, "class=ttl>", "<") else { continue };
        let Some(thumb) = img_src(tile) else { continue };
        let Some(id) = media_id(&thumb) else { continue };
        let href = attr_after(tile, "href=").unwrap_or_default();
        let quality = text_between(tile, "class=frm>", "<").unwrap_or_default().trim().to_string();

        // `/dl/hd/<id>/` exists for every wallpaper; `/dl/4k/<id>/` only
        // for the ones the tile badges "4K".
        let mut variants = vec![Variant { label: "HD".into(), url: format!("{SITE}/dl/hd/{id}/") }];
        if quality.contains("4K") {
            variants.push(Variant { label: "4K".into(), url: format!("{SITE}/dl/4k/{id}/") });
        }

        items.push(OnlineItem {
            id: id.clone(),
            source: SOURCE_MOTIONBGS.to_string(),
            source_name: "MotionBGS".to_string(),
            title: decode_entities(title.trim()),
            thumb_url: absolute(&thumb),
            variants,
            page_url: absolute(&href),
        });
    }
    items
}

fn text_between<'a>(haystack: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let from = haystack.find(start)? + start.len();
    let rest = &haystack[from..];
    Some(&rest[..rest.find(end)?])
}

/// Reads an attribute value that may be unquoted (`href=/x`) or quoted.
fn attr_after(tag: &str, name: &str) -> Option<String> {
    let from = tag.find(name)? + name.len();
    let rest = &tag[from..];
    let value = match rest.chars().next()? {
        quote @ ('"' | '\'') => rest[1..].split(quote).next()?,
        _ => rest.split([' ', '>']).next()?,
    };
    Some(value.to_string())
}

fn img_src(tile: &str) -> Option<String> {
    let img = &tile[tile.find("<img ")?..];
    attr_after(img.split('>').next()?, "src=")
}

fn media_id(path: &str) -> Option<String> {
    let rest = &path[path.find("/media/")? + "/media/".len()..];
    let id: String = rest.chars().take_while(char::is_ascii_digit).collect();
    (!id.is_empty()).then_some(id)
}

fn absolute(path: &str) -> String {
    if path.starts_with("http") {
        path.to_string()
    } else {
        format!("{SITE}{path}")
    }
}

fn decode_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<nav><a title=Home href=/>Home</a></nav><div class=tmb><a title="Wandering Samurai live wallpaper" href=/wandering-samurai> <figure><picture><source srcset=/i/c/364x205/media/10118/wandering-samurai.3840x2160.jpg.webp type=image/webp><img alt="x" height=205 src=/i/c/364x205/media/10118/wandering-samurai.3840x2160.jpg width=364></picture></figure> <span class=ttl>Wandering Samurai</span> <span class=frm> 4K </span> </a><a title="Tom &amp; Jerry live wallpaper" href=/tom-jerry> <figure><img src=/i/c/364x205/media/9/tj.jpg></figure> <span class=ttl>Tom &amp; Jerry</span> <span class=frm> HD </span> </a></div>"#;

    #[test]
    fn parses_tiles_and_skips_non_tiles() {
        let items = parse_listing(SAMPLE);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "10118");
        assert_eq!(items[0].title, "Wandering Samurai");
        assert_eq!(items[0].variants.len(), 2);
        assert_eq!(items[0].variant("HD").unwrap().url, "https://motionbgs.com/dl/hd/10118/");
        assert_eq!(items[0].variant("4K").unwrap().url, "https://motionbgs.com/dl/4k/10118/");
        assert_eq!(items[1].variants.len(), 1);
        assert_eq!(items[0].page_url, "https://motionbgs.com/wandering-samurai");
        assert_eq!(items[0].thumb_url, "https://motionbgs.com/i/c/364x205/media/10118/wandering-samurai.3840x2160.jpg");
        assert_eq!(items[1].title, "Tom & Jerry");
    }

    #[test]
    fn builds_page_urls() {
        assert_eq!(page_url("", 1, None), "https://motionbgs.com/");
        assert_eq!(page_url("", 3, None), "https://motionbgs.com/3/");
        assert_eq!(page_url("dark forest", 1, None), "https://motionbgs.com/search?q=dark+forest");
        assert_eq!(page_url("rain", 2, Some("https://motionbgs.com/tag:rain/")), "https://motionbgs.com/tag:rain/2/");
    }

    #[test]
    fn normalizes_search_redirects() {
        assert_eq!(normalize_base("https://motionbgs.com/tag:rain/").as_deref(), Some("https://motionbgs.com/tag:rain/"));
        assert_eq!(normalize_base("https://motionbgs.com/search?q=dark+forest"), None);
        assert_eq!(normalize_base("https://motionbgs.com/"), None);
    }
}
