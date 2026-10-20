//! Saved search results, so the Online page can show something the moment
//! the app starts (and when offline) instead of waiting on the network
//! every time. One small JSON file; entries are the merged result list for
//! one (query, enabled sources, mature-filter) combination.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use paperloom_common::online_cache_dir;
use serde::{Deserialize, Serialize};

use crate::source::OnlineItem;

/// Oldest entries are dropped past this many saved searches.
const MAX_ENTRIES: usize = 16;

#[derive(Default, Serialize, Deserialize)]
struct Store {
    entries: BTreeMap<String, Entry>,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    saved_at: u64,
    items: Vec<OnlineItem>,
}

pub struct SavedListing {
    pub items: Vec<OnlineItem>,
    /// How long ago it was saved.
    pub age: Duration,
}

fn path() -> PathBuf {
    online_cache_dir().join("listings.json")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

fn read() -> Store {
    std::fs::read_to_string(path()).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

pub fn load(key: &str) -> Option<SavedListing> {
    let entry = read().entries.remove(key)?;
    if entry.items.is_empty() {
        return None;
    }
    Some(SavedListing { items: entry.items, age: Duration::from_secs(now().saturating_sub(entry.saved_at)) })
}

/// Best effort: a failed write just means no saved results next time.
pub fn save(key: &str, items: &[OnlineItem]) {
    if items.is_empty() {
        return;
    }
    let mut store = read();
    store.entries.insert(key.to_string(), Entry { saved_at: now(), items: items.to_vec() });
    while store.entries.len() > MAX_ENTRIES {
        let Some(oldest) = store.entries.iter().min_by_key(|(_, entry)| entry.saved_at).map(|(key, _)| key.clone()) else { break };
        store.entries.remove(&oldest);
    }

    let path = path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(text) = serde_json::to_string(&store) else { return };
    // Write-then-rename so a crash never leaves half a file.
    let temp = path.with_extension("tmp");
    if std::fs::write(&temp, text).is_ok() {
        let _ = std::fs::rename(&temp, &path);
    }
}
