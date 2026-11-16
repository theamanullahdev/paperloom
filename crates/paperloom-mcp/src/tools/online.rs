//! The online library: search the user's enabled sources, download a result.

use std::collections::HashMap;

use paperloom_common::Library;
use paperloom_fetch::OnlineItem;
use serde_json::{json, Value};

use super::library::After;
use super::{Kind, Tool};
use crate::args;

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "search_online",
            about: "Search the wallpaper sites switched on in Paperloom's Sources. Each result can be handed to download_online.",
            kind: Kind::Read,
            fields: &[("query", "string", "What to look for; empty = the latest wallpapers.", false), ("limit", "integer", "How many results (default 20, at most 60).", false)],
            run: search,
        },
        Tool {
            name: "download_online",
            about: "Download one search_online result into the library.",
            kind: Kind::Change,
            fields: &[
                ("item", "object", "One entry of search_online's “results”, exactly as it was returned.", true),
                ("quality", "string", "“HD” (default) or “4K” when the result offers it.", false),
                ("collection", "string", "Also put it in this collection (id or name; a new name creates the collection).", false),
                ("set_as_wallpaper", "boolean", "Put it on the desktop when it has downloaded.", false),
                ("wait_seconds", "integer", "How long to wait before answering (default 40, at most 110). A longer download keeps going; check it with list_downloads.", false),
            ],
            run: download,
        },
        Tool { name: "list_downloads", about: "Downloads started through import_url and download_online since MCP was switched on, with their progress.", kind: Kind::Read, fields: &[], run: |_| Ok(json!({ "downloads": crate::jobs::list() })) },
    ]
}

fn search(args: &Value) -> Result<Value, String> {
    let settings = paperloom_common::load();
    let query = args::text(args, "query").unwrap_or_default();
    let page = paperloom_fetch::fetch_page(&settings.sources, &query, 1, &HashMap::new(), settings.allow_mature).map_err(|err| format!("The search didn't work: {err}"))?;
    let library = Library::load_manifest();
    let limit = (args::whole(args, "limit").unwrap_or(20) as usize).clamp(1, 60);
    let results: Vec<Value> = page
        .items
        .iter()
        .take(limit)
        .map(|item| {
            let mut value = serde_json::to_value(item).unwrap_or_default();
            value["in_library"] = json!(library.contains_source(&item.key()));
            value
        })
        .collect();
    Ok(json!({ "results": results, "notices": page.notices }))
}

fn download(args: &Value) -> Result<Value, String> {
    let mut given = args.get("item").cloned().ok_or("Missing “item” (one result of search_online).")?;
    // A form or a cautious agent may send the object as text.
    if let Some(text) = given.as_str() {
        given = serde_json::from_str(text).map_err(|err| format!("“item” isn't valid JSON: {err}"))?;
    }
    if let Some(object) = given.as_object_mut() {
        object.remove("in_library");
    }
    let item: OnlineItem = serde_json::from_value(given).map_err(|err| format!("“item” isn't a search_online result: {err}"))?;
    let quality = args::text(args, "quality").unwrap_or_else(|| "HD".to_string()).to_uppercase();
    let variant = item
        .variant(&quality)
        .or_else(|| item.variants.first())
        .ok_or("That result has nothing to download.")?
        .clone();
    if Library::load_manifest().contains_source(&item.key()) {
        return Ok(json!({ "state": "done", "message": "Already in the library.", "title": item.title }));
    }
    let id = crate::jobs::start(crate::jobs::Spec {
        source: item.source.clone(),
        url: variant.url,
        title: item.title.clone(),
        after: After {
            name: Some(item.title.clone()),
            collection: args::text(args, "collection"),
            set_as_wallpaper: args::flag(args, "set_as_wallpaper").unwrap_or(false),
            source_id: Some(item.key()),
            origin: Some(item.origin()),
        },
    });
    Ok(crate::jobs::wait(id, args::whole(args, "wait_seconds").unwrap_or(40).min(110)))
}
