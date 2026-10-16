//! Pixabay video API (https://pixabay.com/api/docs/#api_search_videos).
//! Needs a free API key passed as `key=`.

use serde::Deserialize;

use crate::http::{self, FetchError};
use crate::source::{Listing, OnlineItem, Source, Variant};
use paperloom_common::SOURCE_PIXABAY;

const PER_PAGE: u32 = 24;

pub struct Pixabay {
    pub key: String,
}

#[derive(Deserialize)]
struct Response {
    #[serde(rename = "totalHits", default)]
    total_hits: u32,
    #[serde(default)]
    hits: Vec<Hit>,
}

#[derive(Deserialize)]
struct Hit {
    id: u64,
    #[serde(rename = "pageURL")]
    page_url: String,
    #[serde(default)]
    tags: String,
    #[serde(default)]
    picture_id: String,
    videos: Renditions,
}

#[derive(Deserialize, Default)]
struct Renditions {
    large: Option<Rendition>,
    medium: Option<Rendition>,
}

#[derive(Deserialize)]
struct Rendition {
    url: String,
    #[serde(default)]
    width: u32,
}

impl Source for Pixabay {
    fn fetch(&self, query: &str, page: u32, _base: Option<&str>) -> Result<Listing, FetchError> {
        if self.key.trim().is_empty() {
            return Err(FetchError::Permanent("add your Pixabay API key in Sources".into()));
        }
        let url = format!(
            "https://pixabay.com/api/videos/?key={}&q={}&page={page}&per_page={PER_PAGE}",
            crate::motionbgs::url_encode(self.key.trim()),
            crate::motionbgs::url_encode(query.trim())
        );
        let (body, _) = http::get_text(&url)?;
        parse(&body, page)
    }
}

pub(crate) fn parse(body: &str, page: u32) -> Result<Listing, FetchError> {
    let response: Response = serde_json::from_str(body).map_err(|err| FetchError::Permanent(format!("unexpected Pixabay response: {err}")))?;
    let items = response
        .hits
        .into_iter()
        .filter_map(|hit| {
            let usable = |r: Option<Rendition>| r.filter(|r| !r.url.is_empty());
            let (large, medium) = (usable(hit.videos.large), usable(hit.videos.medium));

            // "large" is 4K when the clip has it (else it repeats 1080p).
            let mut variants = Vec::new();
            match (large, medium) {
                (Some(large), medium) if large.width >= 3200 => {
                    if let Some(medium) = medium {
                        variants.push(Variant { label: "HD".into(), url: medium.url });
                    }
                    variants.push(Variant { label: "4K".into(), url: large.url });
                }
                (large, medium) => {
                    if let Some(file) = medium.or(large) {
                        variants.push(Variant { label: "HD".into(), url: file.url });
                    }
                }
            }
            if variants.is_empty() {
                return None;
            }

            let title = hit.tags.split(',').map(str::trim).filter(|t| !t.is_empty()).take(3).collect::<Vec<_>>().join(", ");
            Some(OnlineItem {
                id: hit.id.to_string(),
                source: SOURCE_PIXABAY.to_string(),
                source_name: "Pixabay".to_string(),
                title: if title.is_empty() { format!("Pixabay {}", hit.id) } else { title },
                thumb_url: format!("https://i.vimeocdn.com/video/{}_640x360.jpg", hit.picture_id),
                variants,
                page_url: hit.page_url,
            })
        })
        .collect();
    Ok(Listing { items, base: String::new(), has_more: page * PER_PAGE < response.total_hits })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hits_with_and_without_4k() {
        let body = r#"{"totalHits":100,"hits":[
          {"id":1,"pageURL":"https://pixabay.com/videos/a-1/","tags":"sea, waves, blue, extra","picture_id":"123",
           "videos":{"large":{"url":"https://v/4k.mp4","width":3840},"medium":{"url":"https://v/hd.mp4","width":1920}}},
          {"id":2,"pageURL":"p","tags":"","picture_id":"9",
           "videos":{"large":{"url":"https://v/same.mp4","width":1920},"medium":{"url":"https://v/same.mp4","width":1920}}}]}"#;
        let listing = parse(body, 1).unwrap();
        assert!(listing.has_more);
        assert_eq!(listing.items[0].title, "sea, waves, blue");
        assert_eq!(listing.items[0].variants.iter().map(|v| v.label.as_str()).collect::<Vec<_>>(), ["HD", "4K"]);
        assert_eq!(listing.items[1].variants.len(), 1);
        assert_eq!(listing.items[1].title, "Pixabay 2");
    }
}
