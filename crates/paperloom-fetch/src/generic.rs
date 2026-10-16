//! User-added sources: a page (or JSON API) URL that contains links to
//! `.mp4` files. Deliberately simple — it finds every MP4 URL in the
//! response and lists them — so it works for "a page full of videos" and
//! for most JSON listing APIs without per-site code. No paging.
//!
//! The URL may contain `{q}`, replaced by the search text; without it the
//! search text filters the found titles instead. An API key, if set, is
//! sent as `Authorization: Bearer <key>`.

use crate::http::{self, FetchError};
use crate::source::{Listing, OnlineItem, Source, Variant};

pub struct PageSource {
    pub id: String,
    pub name: String,
    pub url: String,
    pub key: String,
}

impl Source for PageSource {
    fn fetch(&self, query: &str, page: u32, _base: Option<&str>) -> Result<Listing, FetchError> {
        if page > 1 {
            return Ok(Listing::default());
        }
        let url = self.url.trim();
        if url.is_empty() {
            return Err(FetchError::Permanent("no URL set for this source".into()));
        }
        let templated = url.contains("{q}");
        let url = url.replace("{q}", &crate::motionbgs::url_encode(query.trim()));

        let bearer = format!("Bearer {}", self.key.trim());
        let headers: Vec<(&str, &str)> = if self.key.trim().is_empty() { Vec::new() } else { vec![("Authorization", bearer.as_str())] };
        let (body, final_url) = http::get_text_with(&url, &headers)?;

        let needle = query.trim().to_lowercase();
        let items = find_mp4_links(&body, &final_url)
            .into_iter()
            .map(|link| OnlineItem {
                id: format!("{:016x}", fnv1a(&link)),
                source: self.id.clone(),
                source_name: self.name.clone(),
                title: title_from_link(&link),
                thumb_url: String::new(),
                variants: vec![Variant { label: "HD".into(), url: link }],
                page_url: final_url.clone(),
            })
            .filter(|item| templated || needle.is_empty() || item.title.to_lowercase().contains(&needle))
            .collect();
        Ok(Listing { items, base: String::new(), has_more: false })
    }
}

fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3))
}

/// Every distinct `.mp4` URL in `body`, made absolute against `page_url`.
pub(crate) fn find_mp4_links(body: &str, page_url: &str) -> Vec<String> {
    // JSON escapes slashes; undo that so the URLs read normally.
    let body = body.replace("\\/", "/");
    let mut links: Vec<String> = Vec::new();

    for (end, _) in body.match_indices(".mp4") {
        let start = body[..end].rfind(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '(' | ')' | '<' | '>' | '=' | ',')).map_or(0, |i| i + 1);
        let link = &body[start..end + 4];
        if link.len() > 4 && !link.contains(['{', '}']) {
            let absolute = absolute_url(link, page_url);
            if !links.contains(&absolute) {
                links.push(absolute);
            }
        }
    }
    links
}

fn absolute_url(link: &str, page_url: &str) -> String {
    if link.starts_with("http://") || link.starts_with("https://") {
        return link.to_string();
    }
    let scheme_end = page_url.find("://").map_or(0, |i| i + 3);
    let origin_end = page_url[scheme_end..].find('/').map_or(page_url.len(), |i| scheme_end + i);
    if let Some(rest) = link.strip_prefix("//") {
        return format!("{}://{rest}", &page_url[..scheme_end.saturating_sub(3)]);
    }
    if link.starts_with('/') {
        return format!("{}{link}", &page_url[..origin_end]);
    }
    let dir_end = page_url[..page_url.find(['?', '#']).unwrap_or(page_url.len())].rfind('/').map_or(origin_end, |i| i.max(origin_end));
    format!("{}/{link}", &page_url[..dir_end])
}

fn title_from_link(link: &str) -> String {
    let file = link.split(['?', '#']).next().unwrap_or(link).rsplit('/').next().unwrap_or("video");
    let stem = file.strip_suffix(".mp4").unwrap_or(file);
    let words: Vec<&str> = stem.split(['-', '_', '.', '+', ' ']).filter(|w| !w.is_empty()).collect();
    if words.is_empty() { "Video".to_string() } else { words.join(" ") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_and_resolves_links() {
        let html = r#"<video src="/media/a-b.mp4"></video><a href='https://cdn.x/c_d.mp4?x=1'>c</a>
            {"u":"https:\/\/cdn.y\/e.mp4"} <source src=rel/f.mp4 > <a href="/media/a-b.mp4">dup</a>"#;
        let links = find_mp4_links(html, "https://site.test/gallery/index.html");
        assert_eq!(
            links,
            [
                "https://site.test/media/a-b.mp4",
                "https://cdn.x/c_d.mp4",
                "https://cdn.y/e.mp4",
                "https://site.test/gallery/rel/f.mp4",
            ]
        );
    }

    #[test]
    fn titles_come_from_file_names() {
        assert_eq!(title_from_link("https://x/y/neon-city_night.mp4?token=1"), "neon city night");
    }
}
