//! Searches every enabled source at once and merges the results.

use std::collections::HashMap;

use paperloom_common::SourceSetting;

use crate::http::FetchError;
use crate::source::{self, OnlineItem};

/// Where a source left off, so "load more" asks each source for its own
/// next page.
#[derive(Debug, Clone, Default)]
pub struct Cursor {
    pub base: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Page {
    pub items: Vec<OnlineItem>,
    pub cursors: HashMap<String, Cursor>,
    /// One line per source that failed, e.g. "Pexels: add your API key in
    /// Sources". The page still carries whatever the others returned.
    pub notices: Vec<String>,
    pub has_more: bool,
}

/// Fetches page `page` of `query` from all enabled sources in parallel.
/// `cursors` is the previous page's `Page::cursors` (empty for page 1).
pub fn fetch_page(sources: &[SourceSetting], query: &str, page: u32, cursors: &HashMap<String, Cursor>) -> Result<Page, FetchError> {
    // One cheap check up front, so "offline" is reported immediately
    // instead of after every source times out.
    if !crate::net::is_online() {
        return Err(FetchError::Offline);
    }

    let active: Vec<&SourceSetting> = sources
        .iter()
        .filter(|setting| setting.enabled)
        .filter(|setting| page == 1 || cursors.get(&setting.id).is_some_and(|cursor| cursor.has_more))
        .collect();
    if page == 1 && active.is_empty() {
        return Err(FetchError::Permanent("No sources are turned on. Enable one in Sources.".into()));
    }

    let results: Vec<(String, String, Result<source::Listing, FetchError>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = active
            .iter()
            .map(|setting| {
                let base = cursors.get(&setting.id).and_then(|cursor| cursor.base.clone());
                scope.spawn(move || {
                    let source = source::build(setting);
                    (setting.id.clone(), setting.name.clone(), source.fetch(query, page, base.as_deref()))
                })
            })
            .collect();
        handles.into_iter().filter_map(|handle| handle.join().ok()).collect()
    });

    let mut merged = Page::default();
    let mut lists: Vec<Vec<OnlineItem>> = Vec::new();
    let mut first_error = None;

    for (id, name, result) in results {
        match result {
            Ok(listing) => {
                merged.cursors.insert(id, Cursor { base: Some(listing.base).filter(|base| !base.is_empty()), has_more: listing.has_more });
                merged.has_more |= listing.has_more;
                lists.push(listing.items);
            }
            Err(err) => {
                merged.notices.push(format!("{name}: {err}"));
                first_error.get_or_insert(err);
            }
        }
    }

    if lists.is_empty() {
        if let Some(err) = first_error {
            return Err(err);
        }
    }

    // Round-robin so one big source doesn't bury the others.
    let longest = lists.iter().map(Vec::len).max().unwrap_or(0);
    let mut seen = std::collections::HashSet::new();
    for index in 0..longest {
        for list in &lists {
            if let Some(item) = list.get(index) {
                if seen.insert(item.key()) {
                    merged.items.push(item.clone());
                }
            }
        }
    }
    Ok(merged)
}
