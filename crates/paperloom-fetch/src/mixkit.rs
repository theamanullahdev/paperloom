//! Mixkit (free stock video). Its listing pages are server-rendered cards
//! (`data-algolia-analytics-object-id="video-<id>"`) with the title, a
//! thumbnail and the page link, and every video has a direct 720p file at
//! `assets.mixkit.co/videos/<id>/<id>-720.mp4` — the only rendition the
//! site serves without a login, so that is what "HD" means here.

use crate::generic::decode_entities;
use crate::http::{self, FetchError};
use crate::motionbgs::url_encode;
use crate::source::{Listing, OnlineItem, Source, Variant};
use paperloom_common::SOURCE_MIXKIT;

const SITE: &str = "https://mixkit.co";

pub struct Mixkit;

impl Source for Mixkit {
    fn fetch(&self, query: &str, page: u32, _base: Option<&str>) -> Result<Listing, FetchError> {
        let url = page_url(query, page);
        let (html, _) = http::get_text(&url)?;
        Ok(Listing { items: parse_listing(&html), base: String::new(), has_more: html.contains(&format!("page={}", page + 1)) })
    }
}

/// Mixkit's search is its tag pages: `/free-stock-video/<words-joined-by-dashes>/`.
fn page_url(query: &str, page: u32) -> String {
    let slug: String = query
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-");
    let slug = if slug.is_empty() { "abstract".to_string() } else { slug };
    if page <= 1 {
        format!("{SITE}/free-stock-video/{}/", url_encode(&slug))
    } else {
        format!("{SITE}/free-stock-video/{}/?page={page}", url_encode(&slug))
    }
}

pub(crate) fn parse_listing(html: &str) -> Vec<OnlineItem> {
    const MARKER: &str = "data-algolia-analytics-object-id=\"video-";
    html.split(MARKER)
        .skip(1)
        .filter_map(|card| {
            let id: String = card.chars().take_while(char::is_ascii_digit).collect();
            if id.is_empty() {
                return None;
            }
            // A card ends where the next one starts; everything we need is
            // near its top.
            let title = text_between(card, "overlay-video-title\">", "</span>")?.trim().to_string();
            let thumb = text_between(card, "<img src=\"", "\"")?.to_string();
            let href = text_between(card, "<a href=\"/free-stock-video/", "\"").map(|path| format!("{SITE}/free-stock-video/{path}"));

            Some(OnlineItem {
                source: SOURCE_MIXKIT.to_string(),
                source_name: "Mixkit".to_string(),
                title: decode_entities(&title),
                thumb_url: thumb,
                variants: vec![Variant { label: "HD".into(), url: format!("https://assets.mixkit.co/videos/{id}/{id}-720.mp4") }],
                page_url: href.unwrap_or_else(|| format!("{SITE}/free-stock-video/")),
                id,
            })
        })
        .collect()
}

fn text_between<'a>(haystack: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let from = haystack.find(start)? + start.len();
    let rest = &haystack[from..];
    Some(&rest[..rest.find(end)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARD: &str = r#"<div class="item-grid-card" data-algolia-analytics-object-id="video-1751" > <div class="x"><picture><source srcset="https://mixkit-resized.envatousercontent.com/a.jpg" type="image/avif"/> <img src="https://assets.mixkit.co/videos/1751/1751-thumb-360-0.jpg" class="item-grid-video-player__thumb" alt="Rain in a fountain, slow motion shot." width="460"></picture></div>
        <video src="https://assets.mixkit.co/videos/1751/1751-360.mp4" playsinline></video>
        <span class="item-grid-video-player__overlay-video-title"> Rain in a fountain, slow motion shot </span>
        <a href="/free-stock-video/rain-in-a-fountain-slow-motion-shot-1751/" class="item-grid-video-player__overlay-link">x</a></div>
        <div class="item-grid-card" data-algolia-analytics-object-id="video-9" ><img src="https://assets.mixkit.co/videos/9/9-thumb-360-0.jpg"><span class="item-grid-video-player__overlay-video-title">Tom &amp; Jerry</span></div>"#;

    #[test]
    fn parses_cards_with_direct_hd_files() {
        let items = parse_listing(CARD);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "1751");
        assert_eq!(items[0].title, "Rain in a fountain, slow motion shot");
        assert_eq!(items[0].thumb_url, "https://assets.mixkit.co/videos/1751/1751-thumb-360-0.jpg");
        assert_eq!(items[0].variant("HD").unwrap().url, "https://assets.mixkit.co/videos/1751/1751-720.mp4");
        assert_eq!(items[0].page_url, "https://mixkit.co/free-stock-video/rain-in-a-fountain-slow-motion-shot-1751/");
        assert_eq!(items[1].title, "Tom & Jerry");
    }

    #[test]
    fn builds_tag_urls() {
        assert_eq!(page_url("", 1), "https://mixkit.co/free-stock-video/abstract/");
        assert_eq!(page_url("Neon City!", 1), "https://mixkit.co/free-stock-video/neon-city/");
        assert_eq!(page_url("rain", 3), "https://mixkit.co/free-stock-video/rain/?page=3");
    }
}
