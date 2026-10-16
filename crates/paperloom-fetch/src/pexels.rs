//! Pexels video API (https://www.pexels.com/api/documentation/#videos).
//! Needs a free API key sent in the `Authorization` header.

use serde::Deserialize;

use crate::http::{self, FetchError};
use crate::source::{Listing, OnlineItem, Source, Variant};
use paperloom_common::SOURCE_PEXELS;

pub struct Pexels {
    pub key: String,
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    videos: Vec<Video>,
    next_page: Option<String>,
}

#[derive(Deserialize)]
struct Video {
    id: u64,
    url: String,
    image: String,
    #[serde(default)]
    video_files: Vec<File>,
}

#[derive(Deserialize)]
struct File {
    width: Option<u32>,
    link: String,
    file_type: Option<String>,
}

impl Source for Pexels {
    fn fetch(&self, query: &str, page: u32, _base: Option<&str>) -> Result<Listing, FetchError> {
        if self.key.trim().is_empty() {
            return Err(FetchError::Permanent("add your Pexels API key in Sources".into()));
        }
        let url = if query.trim().is_empty() {
            format!("https://api.pexels.com/videos/popular?per_page=24&page={page}")
        } else {
            format!("https://api.pexels.com/videos/search?query={}&per_page=24&page={page}&orientation=landscape", crate::motionbgs::url_encode(query.trim()))
        };
        let (body, _) = http::get_text_with(&url, &[("Authorization", self.key.trim())])?;
        parse(&body)
    }
}

pub(crate) fn parse(body: &str) -> Result<Listing, FetchError> {
    let response: Response = serde_json::from_str(body).map_err(|err| FetchError::Permanent(format!("unexpected Pexels response: {err}")))?;
    let items = response
        .videos
        .into_iter()
        .filter_map(|video| {
            let mp4s = || video.video_files.iter().filter(|f| f.file_type.as_deref().is_none_or(|t| t == "video/mp4"));
            // HD = the widest rendition up to 2560px; 4K = anything >= 3200px.
            let hd = mp4s().filter(|f| (1280..=2560).contains(&f.width.unwrap_or(0))).max_by_key(|f| f.width.unwrap_or(0));
            let uhd = mp4s().filter(|f| f.width.unwrap_or(0) >= 3200).max_by_key(|f| f.width.unwrap_or(0));

            let mut variants = Vec::new();
            if let Some(file) = hd {
                variants.push(Variant { label: "HD".into(), url: file.link.clone() });
            }
            if let Some(file) = uhd {
                variants.push(Variant { label: "4K".into(), url: file.link.clone() });
            }
            if variants.is_empty() {
                return None;
            }

            Some(OnlineItem {
                id: video.id.to_string(),
                source: SOURCE_PEXELS.to_string(),
                source_name: "Pexels".to_string(),
                title: title_from_url(&video.url),
                thumb_url: video.image,
                variants,
                page_url: video.url,
            })
        })
        .collect();
    Ok(Listing { items, base: String::new(), has_more: response.next_page.is_some() })
}

/// `https://www.pexels.com/video/ocean-waves-crashing-3571264/` ->
/// "Ocean Waves Crashing".
fn title_from_url(url: &str) -> String {
    let slug = url.trim_end_matches('/').rsplit('/').next().unwrap_or("video");
    let words: Vec<&str> = slug.split('-').filter(|w| !w.is_empty() && !w.chars().all(|c| c.is_ascii_digit())).collect();
    let title = words
        .iter()
        .map(|w| {
            let mut chars = w.chars();
            chars.next().map(|first| first.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ");
    if title.is_empty() { "Pexels video".to_string() } else { title }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_videos_and_picks_hd_and_4k() {
        let body = r#"{"page":1,"next_page":"https://api.pexels.com/videos/search?page=2","videos":[
          {"id":3571264,"url":"https://www.pexels.com/video/ocean-waves-crashing-3571264/","image":"https://img/1.jpg",
           "video_files":[
             {"quality":"sd","width":640,"link":"https://v/sd.mp4","file_type":"video/mp4"},
             {"quality":"hd","width":1920,"link":"https://v/hd.mp4","file_type":"video/mp4"},
             {"quality":"uhd","width":3840,"link":"https://v/4k.mp4","file_type":"video/mp4"}]},
          {"id":2,"url":"https://www.pexels.com/video/x-2/","image":"i","video_files":[{"width":640,"link":"l","file_type":"video/mp4"}]}]}"#;
        let listing = parse(body).unwrap();
        assert!(listing.has_more);
        assert_eq!(listing.items.len(), 1, "an item with no HD/4K file is skipped");
        let item = &listing.items[0];
        assert_eq!(item.title, "Ocean Waves Crashing");
        assert_eq!(item.variant("HD").unwrap().url, "https://v/hd.mp4");
        assert_eq!(item.variant("4K").unwrap().url, "https://v/4k.mp4");
    }
}
