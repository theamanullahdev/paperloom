//! Page-reading sources: user-added URLs and the built-in site presets
//! (sites the user adds; every known site has its own reader). Deliberately general, so a new site needs a URL,
//! not code:
//!
//! - If the page links straight to `.mp4` files (a gallery page, most JSON
//!   listing APIs), those are the results.
//! - Otherwise each same-site link wrapping an image (a search result
//!   card: link, title, thumbnail) is a result, and the download step
//!   fetches that wallpaper's own page to find its video (`download::download` calls `best_mp4`).
//!
//! The URL may contain `{q}` (the search text; `{q=abstract}` sets what to
//! use when browsing without one) and `{page}` (page number; without it
//! there is only page 1). An API key, if set, is sent as
//! `Authorization: Bearer <key>`.

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
        let template = self.url.trim();
        if template.is_empty() {
            return Err(FetchError::Permanent("no URL set for this source".into()));
        }
        let paged = template.contains("{page}");
        if page > 1 && !paged {
            return Ok(Listing::default());
        }
        let templated = template.contains("{q");
        let url = fill_template(template, query, page);

        let bearer = format!("Bearer {}", self.key.trim());
        let headers: Vec<(&str, &str)> = if self.key.trim().is_empty() { Vec::new() } else { vec![("Authorization", bearer.as_str())] };
        let (body, final_url) = match http::get_text_with(&url, &headers) {
            // Past the last page, sites answer 404.
            Err(FetchError::Permanent(_)) if page > 1 => return Ok(Listing::default()),
            other => other?,
        };

        let item = |link: String, title: String, thumb: String| OnlineItem {
            id: format!("{:016x}", fnv1a(&link)),
            source: self.id.clone(),
            source_name: self.name.clone(),
            title,
            thumb_url: thumb,
            page_url: if link.to_lowercase().contains(".mp4") { final_url.clone() } else { link.clone() },
            variants: vec![Variant { label: "HD".into(), url: link }],
        };
        let mut items: Vec<OnlineItem> = find_tiles(&body, &final_url).into_iter().map(|tile| item(tile.link, tile.title, tile.thumb)).collect();
        if items.is_empty() {
            items = find_mp4_links(&body, &final_url).into_iter().map(|link| item(link.clone(), title_from_link(&link), String::new())).collect();
        }

        let needle = query.trim().to_lowercase();
        items.retain(|item| templated || needle.is_empty() || item.title.to_lowercase().contains(&needle));
        let has_more = paged && !items.is_empty();
        Ok(Listing { items, base: String::new(), has_more })
    }
}

/// Fills `{q}` / `{q=default}` and `{page}`.
fn fill_template(template: &str, query: &str, page: u32) -> String {
    let mut url = template.replace("{page}", &page.to_string());
    while let Some(start) = url.find("{q") {
        let Some(len) = url[start..].find('}') else { break };
        let default = url[start + 2..start + len].strip_prefix('=').unwrap_or("");
        let text = if query.trim().is_empty() { default.to_string() } else { query.trim().to_string() };
        url.replace_range(start..=start + len, &crate::motionbgs::url_encode(&text));
    }
    url
}

struct Tile {
    link: String,
    title: String,
    thumb: String,
}

/// Search-result cards: `<a href=…>…<img …>…</a>` pointing at another page
/// of the same site. Navigation (home, paging, categories, search, log-in)
/// is skipped; each link counts once.
fn find_tiles(html: &str, page_url: &str) -> Vec<Tile> {
    let host = host_of(page_url);
    let mut tiles: Vec<Tile> = Vec::new();
    for chunk in html.split("<a ").skip(1) {
        let Some(anchor) = chunk.split("</a>").next() else { continue };
        let Some(tag_end) = anchor.find('>') else { continue };
        let (open, inner) = (&anchor[..tag_end], &anchor[tag_end..]);
        let Some(img_at) = inner.find("<img") else { continue };
        let Some(href) = attr(open, "href") else { continue };
        let link = absolute_url(&href, page_url);
        let path = link.split("://").nth(1).and_then(|rest| rest.find('/').map(|i| &rest[i..])).unwrap_or("/");
        let nav = ["/page/", "?s=", "/tag/", "/category/", "/author/", "/search", "login", "/wp-", "#", "/feed"];
        if host_of(&link) != host || path.len() < 3 || nav.iter().any(|word| path.contains(word)) || link.split('?').next() == page_url.split('?').next() {
            continue;
        }
        if tiles.iter().any(|tile| tile.link == link) {
            continue;
        }
        let img = &inner[img_at..inner[img_at..].find('>').map_or(inner.len(), |i| img_at + i)];
        let thumb = ["data-lazy-src", "data-src", "data-original", "src"]
            .iter()
            .filter_map(|name| attr(img, name))
            .find(|src| !src.starts_with("data:"))
            .map(|src| absolute_url(&src, page_url))
            .unwrap_or_default();
        let title = attr(open, "title")
            .or_else(|| attr(img, "alt"))
            .map(|text| decode_entities(&text))
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| title_from_link(&link));
        tiles.push(Tile { link, title: title.trim().to_string(), thumb });
    }
    tiles
}

/// The value of `name=` in a tag: quoted or bare.
fn attr(tag: &str, name: &str) -> Option<String> {
    let mut rest = tag;
    loop {
        let at = rest.find(name)?;
        let before = rest[..at].chars().last();
        let after = &rest[at + name.len()..];
        rest = after;
        if !before.is_none_or(char::is_whitespace) || !after.trim_start().starts_with('=') {
            continue;
        }
        let value = after.trim_start()[1..].trim_start();
        return Some(match value.chars().next()? {
            quote @ ('"' | '\'') => value[1..].split(quote).next()?.to_string(),
            _ => value.split(|c: char| c.is_whitespace() || c == '>').next()?.to_string(),
        });
    }
}

fn host_of(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let host = rest.split(['/', '?', '#']).next().unwrap_or("").to_lowercase();
    host.strip_prefix("www.").map(str::to_string).unwrap_or(host)
}

pub(crate) fn decode_entities(text: &str) -> String {
    text.replace("&amp;", "&").replace("&#039;", "'").replace("&#8217;", "’").replace("&quot;", "\"").replace("&#8211;", "–").replace("&lt;", "<").replace("&gt;", ">")
}

/// Picks the wallpaper's real video out of its page: the best-sounding
/// `.mp4` (largest, not a preview).
pub fn best_mp4(body: &str, page_url: &str) -> Result<String, FetchError> {
    let score = |link: &String| {
        let link = link.to_lowercase();
        let mut score = 0;
        for (word, points) in [("2160", 4), ("4k", 4), ("uhd", 4), ("1440", 3), ("1080", 2), ("large", 2), ("hd", 1), ("download", 1)] {
            if link.contains(word) {
                score += points;
            }
        }
        for word in ["preview", "thumb", "sample", "small", "360", "480"] {
            if link.contains(word) {
                score -= 3;
            }
        }
        score
    };
    find_mp4_links(body, page_url)
        .into_iter()
        .max_by_key(score)
        .ok_or_else(|| FetchError::Permanent("no video found on the wallpaper's page".into()))
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
    fn reads_result_cards_and_templates() {
        let html = r#"<a href="/"><img src="/logo.png"></a>
            <a href="https://site.test/anime/rainy-night-live-wallpaper/" title="Rainy Night &amp; Neon"><img data-src="/t/rain.jpg" src="data:x"></a>
            <a href=/page/2/><img src=/arrow.png></a>
            <a href="https://ads.test/x"><img src="/ad.png"></a>"#;
        let tiles = find_tiles(html, "https://www.site.test/page/1/?s=rain");
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].title, "Rainy Night & Neon");
        assert_eq!(tiles[0].thumb, "https://www.site.test/t/rain.jpg");
        assert_eq!(fill_template("https://x/{q=abstract}/?page={page}", "", 2), "https://x/abstract/?page=2");
        assert_eq!(fill_template("https://x/page/{page}/?s={q}", "neon city", 1), "https://x/page/1/?s=neon+city");
    }

    #[test]
    fn titles_come_from_file_names() {
        assert_eq!(title_from_link("https://x/y/neon-city_night.mp4?token=1"), "neon city night");
    }
}
