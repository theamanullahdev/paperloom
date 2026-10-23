//! Collections that downloaded wallpapers are waiting to join.
//!
//! Importing a share queues downloads that finish later (maybe after a
//! restart). Until each one lands in the library, "put it in collection X"
//! is kept here, by the wallpaper's key, in a small text file next to the
//! library: one `key<TAB>collection` line each.

use std::collections::BTreeMap;
use std::path::PathBuf;

fn path() -> PathBuf {
    paperloom_common::library_dir().join("share-pending.txt")
}

fn load() -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(path()).unwrap_or_default();
    text.lines().filter_map(|line| line.split_once('\t')).map(|(key, name)| (key.to_string(), name.to_string())).collect()
}

fn save(map: &BTreeMap<String, String>) {
    let path = path();
    if map.is_empty() {
        let _ = std::fs::remove_file(path);
        return;
    }
    let text: String = map.iter().map(|(key, name)| format!("{key}\t{name}\n")).collect();
    let _ = std::fs::create_dir_all(paperloom_common::library_dir());
    let _ = std::fs::write(path, text);
}

/// Remembers that each wallpaper in `keys` should join `collection` once downloaded.
pub fn add(keys: &[String], collection: &str) {
    if keys.is_empty() {
        return;
    }
    let name: String = collection.chars().filter(|c| !c.is_control()).collect();
    let mut map = load();
    for key in keys {
        map.insert(key.clone(), name.clone());
    }
    save(&map);
}

/// The collection `key` was promised to, forgotten once asked.
pub fn take(key: &str) -> Option<String> {
    let mut map = load();
    let found = map.remove(key)?;
    save(&map);
    Some(found)
}

/// Forgets a promise (the download was cancelled).
pub fn forget(key: &str) {
    let _ = take(key);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_round_trip() {
        let mut map = BTreeMap::new();
        map.insert("motionbgs.com:1".to_string(), "Rainy evenings".to_string());
        let text: String = map.iter().map(|(k, n)| format!("{k}\t{n}\n")).collect();
        let back: BTreeMap<String, String> = text.lines().filter_map(|l| l.split_once('\t')).map(|(k, n)| (k.to_string(), n.to_string())).collect();
        assert_eq!(map, back);
    }
}
