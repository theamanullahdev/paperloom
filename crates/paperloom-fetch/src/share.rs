//! Sharing wallpapers: a share is a small list of *where each wallpaper
//! comes from* (source, id, title, public download links), never video data
//! and never anything private. A friend's Paperloom downloads the same files
//! again from the same sources.
//!
//! On the wire a share is JSON, deflated and base64url-encoded behind a
//! `paperloom-share:1:` prefix — short enough to paste in a chat, and the
//! same text is what a `.paperloomshare` file holds.

use std::io::{Read, Write};

use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use paperloom_common::{LibraryEntry, Origin, SourceSetting};
use serde::{Deserialize, Serialize};

use crate::source::OnlineItem;

const PREFIX: &str = "paperloom-share:1:";
const VERSION: u32 = 1;
/// A share larger than this (items) or decoded size is refused.
pub const MAX_ITEMS: usize = 500;
const MAX_DECODED: u64 = 4 * 1024 * 1024;
const MAX_TITLE: usize = 200;
const MAX_ID: usize = 200;
/// Past this many wallpapers a file is friendlier than a pasted code.
pub const CODE_COMFORT: usize = 60;

/// Sites a share may point at (plus the hosts of the receiver's own sources).
const KNOWN_HOSTS: [&str; 5] = ["motionbgs.com", "mixkit.co", "pexels.com", "pixabay.com", "wallpaperwaves.com"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Items,
    Collection,
    Library,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// Source id (`motionbgs.com`).
    pub s: String,
    /// Source's display name.
    pub n: String,
    /// Source-native id.
    pub i: String,
    pub t: String,
    pub th: String,
    pub p: String,
    /// (label, url).
    pub v: Vec<(String, String)>,
}

impl Item {
    pub fn key(&self) -> String {
        format!("{}:{}", self.s, self.i)
    }

    pub fn online_item(&self) -> OnlineItem {
        OnlineItem {
            id: self.i.clone(),
            source: self.s.clone(),
            source_name: self.n.clone(),
            title: self.t.clone(),
            thumb_url: self.th.clone(),
            variants: self.v.iter().map(|(label, url)| crate::source::Variant { label: label.clone(), url: url.clone() }).collect(),
            page_url: self.p.clone(),
        }
    }

    pub fn has_4k(&self) -> bool {
        self.v.iter().any(|(label, _)| label == "4K")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Share {
    pub v: u32,
    pub kind: Kind,
    #[serde(default)]
    pub name: String,
    /// How many wallpapers were left out when this was made (imported from
    /// the sender's own files, or hidden).
    #[serde(default)]
    pub left_out: u32,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareError {
    NotAShare,
    Damaged,
    TooNew,
    TooBig,
    Empty,
}

impl std::fmt::Display for ShareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ShareError::NotAShare => "That isn't a Paperloom share.",
            ShareError::Damaged => "That share looks damaged or cut off.",
            ShareError::TooNew => "That share was made by a newer Paperloom. Update the app to open it.",
            ShareError::TooBig => "That share is too big to open.",
            ShareError::Empty => "That share has no wallpapers I can download.",
        })
    }
}

/// What a library entry can be shared as: its kept origin, or, for entries
/// downloaded before origins were kept, what the source's id alone gives.
pub fn origin_of(entry: &LibraryEntry) -> Option<Origin> {
    if let Some(origin) = &entry.origin {
        return Some(origin.clone());
    }
    let key = entry.source_id.as_deref()?;
    let (source, id) = key.split_once(':')?;
    let (name, thumb, page, variants) = match source {
        paperloom_common::SOURCE_MOTIONBGS => ("MotionBGs", String::new(), "https://motionbgs.com/".to_string(), vec![("HD".to_string(), format!("https://motionbgs.com/dl/hd/{id}/"))]),
        paperloom_common::SOURCE_MIXKIT => (
            "Mixkit (stock video)",
            String::new(),
            "https://mixkit.co/free-stock-video/".to_string(),
            vec![("HD".to_string(), format!("https://assets.mixkit.co/videos/{id}/{id}-720.mp4"))],
        ),
        _ => return None,
    };
    Some(Origin { source: source.into(), source_name: name.into(), item_id: id.into(), thumb_url: thumb, page_url: page, variants })
}

/// Looks the wallpaper `key` (`source:id`) up on its source by `title`, for
/// downloads made before their origin was kept. `None` when the source
/// doesn't list it under that title.
pub fn find_origin(sources: &[SourceSetting], title: &str, key: &str) -> Option<Origin> {
    let (source_id, item_id) = key.split_once(':')?;
    let mut setting = sources.iter().find(|source| source.id == source_id)?.clone();
    setting.enabled = true;
    let page = crate::fetch_page(&[setting], title, 1, &Default::default(), true).ok()?;
    page.items.into_iter().find(|item| item.id == item_id && item.source == source_id).map(|item| item.origin())
}

impl Share {
    /// Builds a share from library entries. Hidden wallpapers and ones with
    /// no public source are left out and counted.
    pub fn from_entries<'a>(kind: Kind, name: &str, entries: impl IntoIterator<Item = &'a LibraryEntry>) -> Share {
        let mut items = Vec::new();
        let mut left_out = 0u32;
        for entry in entries {
            let origin = if entry.hidden { None } else { origin_of(entry) };
            match origin {
                Some(o) if !o.variants.is_empty() && !items.iter().any(|i: &Item| i.s == o.source && i.i == o.item_id) => items.push(Item {
                    s: o.source,
                    n: o.source_name,
                    i: o.item_id,
                    t: entry.display_name.clone(),
                    th: o.thumb_url,
                    p: o.page_url,
                    v: o.variants,
                }),
                Some(_) => {}
                None => left_out += 1,
            }
        }
        items.truncate(MAX_ITEMS);
        Share { v: VERSION, kind, name: name.trim().chars().take(MAX_TITLE).collect(), left_out, items }
    }

    /// The text to paste or save.
    pub fn encode(&self) -> String {
        let json = serde_json::to_vec(self).unwrap_or_default();
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::best());
        let _ = encoder.write_all(&json);
        let packed = encoder.finish().unwrap_or_default();
        format!("{PREFIX}{}", base64url_encode(&packed))
    }

    /// Reads a share from pasted text or a file's contents, keeping only
    /// items that are safe to download given `hosts` (see [`allowed_hosts`]).
    pub fn decode(text: &str, hosts: &[String]) -> Result<Share, ShareError> {
        let cleaned: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        let body = cleaned.strip_prefix(PREFIX).ok_or_else(|| {
            if cleaned.starts_with("paperloom-share:") {
                ShareError::TooNew
            } else {
                ShareError::NotAShare
            }
        })?;
        let packed = base64url_decode(body).ok_or(ShareError::Damaged)?;
        let mut json = Vec::new();
        DeflateDecoder::new(packed.as_slice()).take(MAX_DECODED + 1).read_to_end(&mut json).map_err(|_| ShareError::Damaged)?;
        if json.len() as u64 > MAX_DECODED {
            return Err(ShareError::TooBig);
        }
        let mut share: Share = serde_json::from_slice(&json).map_err(|_| ShareError::Damaged)?;
        if share.v > VERSION {
            return Err(ShareError::TooNew);
        }
        if share.items.len() > MAX_ITEMS {
            return Err(ShareError::TooBig);
        }
        share.name = clean(&share.name, MAX_TITLE);
        let before = share.items.len();
        share.items.retain_mut(|item| sanitize(item, hosts));
        share.left_out = share.left_out.saturating_add((before - share.items.len()) as u32);
        let mut seen = std::collections::HashSet::new();
        share.items.retain(|item| seen.insert(item.key()));
        if share.items.is_empty() {
            return Err(ShareError::Empty);
        }
        Ok(share)
    }
}

/// Hosts a received share may download from: the known sites plus the
/// receiver's own added sources.
pub fn allowed_hosts(sources: &[SourceSetting]) -> Vec<String> {
    let mut hosts: Vec<String> = KNOWN_HOSTS.iter().map(|h| h.to_string()).collect();
    for source in sources {
        if let Some(host) = host_of(&source.url) {
            hosts.push(host);
        }
    }
    hosts
}

fn clean(text: &str, max: usize) -> String {
    text.chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_string()
}

fn host_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    if host.contains('@') || host.is_empty() {
        return None;
    }
    Some(host.split(':').next()?.to_ascii_lowercase())
}

fn host_allowed(url: &str, hosts: &[String]) -> bool {
    host_of(url).is_some_and(|host| hosts.iter().any(|allowed| host == *allowed || host.ends_with(&format!(".{allowed}"))))
}

/// Cleans one item in place; `false` means drop it.
fn sanitize(item: &mut Item, hosts: &[String]) -> bool {
    item.s = clean(&item.s, MAX_ID);
    item.n = clean(&item.n, MAX_TITLE);
    item.i = clean(&item.i, MAX_ID);
    item.t = clean(&item.t, MAX_TITLE);
    if item.s.is_empty() || item.i.is_empty() || item.i.contains(':') && item.s.contains(':') {
        return false;
    }
    if item.t.is_empty() {
        item.t = item.i.clone();
    }
    if item.n.is_empty() {
        item.n = item.s.clone();
    }
    item.v.retain(|(label, url)| (label == "HD" || label == "4K") && host_allowed(url, hosts));
    if item.v.is_empty() {
        return false;
    }
    if !host_allowed(&item.th, hosts) {
        item.th.clear();
    }
    if !host_allowed(&item.p, hosts) {
        item.p.clear();
    }
    true
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn base64url_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |acc, (i, b)| acc | (*b as u32) << (16 - 8 * i));
        let symbols = chunk.len() + 1;
        for i in 0..symbols {
            out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

fn base64url_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for c in text.trim_end_matches('=').bytes() {
        let value = ALPHABET.iter().position(|&a| a == c)? as u32;
        acc = acc << 6 | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits & 0xff) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosts() -> Vec<String> {
        allowed_hosts(&[])
    }

    fn item(id: &str) -> Item {
        Item {
            s: "motionbgs.com".into(),
            n: "MotionBGs".into(),
            i: id.into(),
            t: format!("Wallpaper {id}"),
            th: "https://motionbgs.com/i/c/364/1.jpg".into(),
            p: "https://motionbgs.com/x/".into(),
            v: vec![("HD".into(), format!("https://motionbgs.com/dl/hd/{id}/"))],
        }
    }

    fn share(items: Vec<Item>) -> Share {
        Share { v: 1, kind: Kind::Items, name: "Rain".into(), left_out: 2, items }
    }

    #[test]
    fn round_trips() {
        let original = share(vec![item("1"), item("2")]);
        let text = original.encode();
        assert!(text.starts_with(PREFIX));
        assert_eq!(Share::decode(&text, &hosts()).unwrap(), original);
    }

    #[test]
    fn survives_chat_line_wrapping() {
        let text = share(vec![item("1")]).encode();
        let wrapped = format!("  {}\n{}  ", &text[..30], &text[30..]);
        assert!(Share::decode(&wrapped, &hosts()).is_ok());
    }

    #[test]
    fn sixty_wallpapers_stay_small() {
        let items: Vec<Item> = (0..60).map(|n| item(&(10000 + n).to_string())).collect();
        assert!(share(items).encode().len() < 6000);
    }

    #[test]
    fn rejects_other_text_and_damage() {
        assert_eq!(Share::decode("hello", &hosts()), Err(ShareError::NotAShare));
        let text = share(vec![item("1")]).encode();
        assert_eq!(Share::decode(&text[..text.len() - 20], &hosts()), Err(ShareError::Damaged));
        assert_eq!(Share::decode("paperloom-share:9:abc", &hosts()), Err(ShareError::TooNew));
    }

    #[test]
    fn drops_links_to_unknown_sites() {
        let mut bad = item("2");
        bad.v = vec![("HD".into(), "https://evil.example/x.mp4".into())];
        let mut half = item("3");
        half.th = "https://evil.example/t.jpg".into();
        half.v.push(("4K".into(), "http://motionbgs.com/dl/4k/3/".into()));
        let got = Share::decode(&share(vec![item("1"), bad, half]).encode(), &hosts()).unwrap();
        assert_eq!(got.items.len(), 2);
        assert_eq!(got.left_out, 3);
        assert!(got.items[1].th.is_empty());
        assert_eq!(got.items[1].v.len(), 1, "plain http is refused");
    }

    #[test]
    fn host_checks_are_exact() {
        let hosts = hosts();
        assert!(host_allowed("https://motionbgs.com/a", &hosts));
        assert!(host_allowed("https://cdn.pixabay.com/a", &hosts));
        assert!(!host_allowed("https://motionbgs.com.evil.io/a", &hosts));
        assert!(!host_allowed("https://user@motionbgs.com/a", &hosts));
        assert!(!host_allowed("https://notmotionbgs.com/a", &hosts));
    }

    #[test]
    fn user_added_sources_are_allowed() {
        let source = SourceSetting { id: "custom-1".into(), name: "X".into(), enabled: true, api_key: String::new(), url: "https://walls.example.org/list".into(), builtin: false };
        assert!(host_allowed("https://walls.example.org/v.mp4", &allowed_hosts(&[source])));
    }

    #[test]
    fn duplicates_collapse() {
        let got = Share::decode(&share(vec![item("1"), item("1")]).encode(), &hosts()).unwrap();
        assert_eq!(got.items.len(), 1);
    }

    #[test]
    fn old_downloads_can_be_shared_by_id() {
        let entry = LibraryEntry {
            id: 1,
            display_name: "Old".into(),
            path: "/x.mp4".into(),
            mode: paperloom_common::ImportMode::default(),
            thumbnail: None,
            preview_frames: vec![],
            preview_version: 0,
            source_id: Some("motionbgs.com:10118".into()),
            hidden: false,
            added: 0,
            optimize: Default::default(),
            fingerprint: None,
            favorite: false,
            tags: vec![],
            origin: None,
        };
        let mut own = entry.clone();
        own.source_id = None;
        let mut hidden = entry.clone();
        hidden.hidden = true;
        let made = Share::from_entries(Kind::Library, "All", [&entry, &own, &hidden]);
        assert_eq!(made.items.len(), 1);
        assert_eq!(made.left_out, 2);
        assert_eq!(made.items[0].v[0].1, "https://motionbgs.com/dl/hd/10118/");
    }
}
