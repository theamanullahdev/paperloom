//! The user's wallpapers: list, look up, import, tag, hide, remove.

use std::path::Path;

use paperloom_common::{Collection, ImportMode, Library, LibraryEntry, Origin};
use serde_json::{json, Value};

use super::{Kind, Tool};
use crate::args;

/// What the app's own import dialog accepts.
const IMPORTABLE: [&str; 10] = ["mp4", "webm", "mkv", "mov", "avi", "gif", "jpg", "jpeg", "png", "webp"];

const WALLPAPER: (&str, &str, &str, bool) = ("wallpaper", "string", "The wallpaper's id or (part of) its name.", true);

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "list_wallpapers",
            about: "The wallpapers in the library, newest last. Filter by text, collection, tag or favorites.",
            kind: Kind::Read,
            fields: &[
                ("query", "string", "Only names or tags containing this text.", false),
                ("collection", "string", "Only this collection (id or name).", false),
                ("tag", "string", "Only wallpapers with this tag.", false),
                ("favorites", "boolean", "Only favorites.", false),
                ("include_hidden", "boolean", "Also list wallpapers in the Hidden section (left out by default).", false),
                ("limit", "integer", "How many to return (default 50, at most 200).", false),
                ("offset", "integer", "Skip this many first (for paging).", false),
            ],
            run: list,
        },
        Tool { name: "get_wallpaper", about: "Everything about one wallpaper: file, tags, collections, where it came from.", kind: Kind::Read, fields: &[WALLPAPER], run: get },
        Tool {
            name: "import_file",
            about: "Add a video or image file on this computer to the library (mp4, webm, mkv, mov, avi, gif, jpg, png, webp). Use this after downloading a wallpaper yourself.",
            kind: Kind::Change,
            fields: &[
                ("path", "string", "Full path of the file.", true),
                ("name", "string", "A nicer name to show than the file name.", false),
                ("collection", "string", "Also put it in this collection (id or name; a new name creates the collection).", false),
                ("set_as_wallpaper", "boolean", "Put it on the desktop right away.", false),
            ],
            run: import_file,
        },
        Tool {
            name: "import_url",
            about: "Download a wallpaper from a link and add it to the library. Works for a direct video link (.mp4) or a page that links one. If Paperloom can't get a video from the link, download the file yourself and call import_file.",
            kind: Kind::Change,
            fields: &[
                ("url", "string", "http(s) link to the video or to its page.", true),
                ("name", "string", "A name for the wallpaper.", false),
                ("collection", "string", "Also put it in this collection (id or name; a new name creates the collection).", false),
                ("set_as_wallpaper", "boolean", "Put it on the desktop when it has downloaded.", false),
                ("wait_seconds", "integer", "How long to wait for the download before answering (default 40, at most 110). A longer download keeps going; check it with list_downloads.", false),
            ],
            run: import_url,
        },
        Tool {
            name: "remove_wallpaper",
            about: "Remove a wallpaper from the library. A copy Paperloom made is deleted; a file that was only linked stays where it is. Cannot be undone.",
            kind: Kind::Remove,
            fields: &[WALLPAPER, ("confirm", "boolean", "Must be true.", true)],
            run: remove,
        },
        Tool { name: "set_favorite", about: "Mark or unmark a wallpaper as a favorite.", kind: Kind::Change, fields: &[WALLPAPER, ("favorite", "boolean", "true = favorite.", true)], run: favorite },
        Tool { name: "set_tags", about: "Replace a wallpaper's tags.", kind: Kind::Change, fields: &[WALLPAPER, ("tags", "list", "The tags it should have (empty list clears them).", true)], run: tags },
        Tool { name: "set_hidden", about: "Move a wallpaper into or out of the Hidden section.", kind: Kind::Change, fields: &[WALLPAPER, ("hidden", "boolean", "true = hidden.", true)], run: hidden },
    ]
}

/// The wallpaper `wanted` names: an id, a file path, an exact name, or text
/// that only one name contains.
pub fn find<'a>(library: &'a Library, wanted: &str) -> Result<&'a LibraryEntry, String> {
    let wanted = wanted.trim();
    if let Some(entry) = wanted.parse::<u64>().ok().and_then(|id| library.entry(id)) {
        return Ok(entry);
    }
    if let Some(entry) = library.entries.iter().find(|entry| entry.path == Path::new(wanted)) {
        return Ok(entry);
    }
    let needle = wanted.to_lowercase();
    let stem = |name: &str| Path::new(name).file_stem().map(|stem| stem.to_string_lossy().to_lowercase()).unwrap_or_default();
    if let Some(entry) = library.entries.iter().find(|entry| entry.display_name.to_lowercase() == needle || stem(&entry.display_name) == needle) {
        return Ok(entry);
    }
    let matches: Vec<&LibraryEntry> = library.entries.iter().filter(|entry| entry.display_name.to_lowercase().contains(&needle)).collect();
    match matches.as_slice() {
        [] => Err(format!("No wallpaper in the library matches “{wanted}”. Use list_wallpapers to see what there is.")),
        [only] => Ok(only),
        many => Err(format!(
            "“{wanted}” matches {} wallpapers: {}. Use the id.",
            many.len(),
            many.iter().take(8).map(|entry| format!("{} (id {})", entry.display_name, entry.id)).collect::<Vec<_>>().join(", ")
        )),
    }
}

/// The collection `wanted` names (id or name, ignoring case).
pub fn find_collection<'a>(library: &'a Library, wanted: &str) -> Result<&'a Collection, String> {
    let wanted = wanted.trim();
    if let Some(collection) = wanted.parse::<u64>().ok().and_then(|id| library.collection(id)) {
        return Ok(collection);
    }
    library
        .collections
        .iter()
        .find(|collection| collection.name.eq_ignore_ascii_case(wanted))
        .ok_or_else(|| format!("There is no collection called “{wanted}”. Use list_collections to see them."))
}

pub fn describe(library: &Library, entry: &LibraryEntry) -> Value {
    json!({
        "id": entry.id,
        "name": entry.display_name,
        "path": entry.path,
        "file_missing": !entry.path.is_file(),
        "favorite": entry.favorite,
        "hidden": entry.hidden,
        "tags": entry.tags,
        "collections": library.collections_of(entry.id).map(|collection| collection.name.clone()).collect::<Vec<_>>(),
        "added_unix": entry.added,
        "source": entry.origin.as_ref().map(|origin| json!({ "site": origin.source_name, "page": origin.page_url })),
    })
}

fn list(args: &Value) -> Result<Value, String> {
    let library = Library::load_manifest();
    let query = args::text(args, "query").map(|query| query.to_lowercase());
    let tag = args::text(args, "tag").map(|tag| tag.trim_start_matches('#').to_lowercase());
    let favorites = args::flag(args, "favorites").unwrap_or(false);
    let include_hidden = args::flag(args, "include_hidden").unwrap_or(false);
    let members = match args::text(args, "collection") {
        Some(wanted) => Some(find_collection(&library, &wanted)?.members.clone()),
        None => None,
    };
    let matching: Vec<&LibraryEntry> = library
        .entries
        .iter()
        .filter(|entry| include_hidden || !entry.hidden)
        .filter(|entry| !favorites || entry.favorite)
        .filter(|entry| members.as_ref().is_none_or(|members| members.contains(&entry.id)))
        .filter(|entry| tag.as_ref().is_none_or(|tag| entry.tags.contains(tag)))
        .filter(|entry| query.as_ref().is_none_or(|query| entry.display_name.to_lowercase().contains(query) || entry.tags.iter().any(|tag| tag.contains(query))))
        .collect();
    let offset = args::whole(args, "offset").unwrap_or(0) as usize;
    let limit = (args::whole(args, "limit").unwrap_or(50) as usize).clamp(1, 200);
    Ok(json!({
        "total": matching.len(),
        "offset": offset,
        "wallpapers": matching.iter().skip(offset).take(limit).map(|entry| describe(&library, entry)).collect::<Vec<_>>(),
    }))
}

fn get(args: &Value) -> Result<Value, String> {
    let library = Library::load_manifest();
    let entry = find(&library, &args::need_text(args, "wallpaper")?)?;
    let mut details = describe(&library, entry);
    details["import_mode"] = json!(format!("{:?}", entry.mode));
    details["optimize"] = json!(format!("{:?}", entry.optimize));
    if let Some(info) = entry.path.is_file().then(|| paperloom_common::media::probe(&entry.path)).flatten() {
        details["video"] = json!(format!("{info:?}"));
    }
    Ok(details)
}

/// What to do with a file once it is in the library.
#[derive(Debug, Clone, Default)]
pub struct After {
    pub name: Option<String>,
    pub collection: Option<String>,
    pub set_as_wallpaper: bool,
    /// For downloads: the library's `source_id` and where it came from.
    pub source_id: Option<String>,
    pub origin: Option<Origin>,
}

/// Adds `path` to the library the way the app's Import does (same folder,
/// same import mode, same duplicate check), then applies `after`.
pub fn add_file(path: &Path, after: &After) -> Result<Value, String> {
    let settings = paperloom_common::load();
    let mut library = Library::load_manifest();

    let existing = paperloom_common::fingerprint(path).and_then(|print| library.entries.iter().find(|entry| entry.fingerprint.as_deref() == Some(print.as_str())).map(|entry| entry.id));
    let (id, already) = match existing {
        Some(id) => (id, true),
        None => {
            let imported = match &after.source_id {
                Some(source_id) => library.import_named(path, settings.import_mode, after.name.as_deref().unwrap_or("wallpaper"), source_id),
                None => library.import(path, settings.import_mode),
            };
            imported.map_err(|err| format!("Couldn't add it to the library: {err}"))?;
            let id = library.entries.last().map(|entry| entry.id).ok_or("The import added nothing.")?;
            if let (None, Some(name)) = (&after.source_id, &after.name) {
                if let Some(entry) = library.entries.iter_mut().find(|entry| entry.id == id) {
                    entry.display_name = name.clone();
                }
                library.save().map_err(|err| format!("Couldn't save the library: {err}"))?;
            }
            if let (Some(source_id), Some(origin)) = (&after.source_id, &after.origin) {
                let _ = library.set_origin(source_id, origin.clone());
            }
            (id, false)
        }
    };

    if let Some(wanted) = &after.collection {
        let collection = match find_collection(&library, wanted) {
            Ok(collection) => collection.id,
            Err(_) => library.create_collection(wanted).map_err(|err| format!("Couldn't create the collection: {err}"))?,
        };
        library.set_in_collection(collection, id, true).map_err(|err| format!("Couldn't add it to the collection: {err}"))?;
    }

    let entry = library.entry(id).ok_or("The wallpaper vanished from the library.")?;
    let mut result = describe(&library, entry);
    result["already_in_library"] = json!(already);
    if after.set_as_wallpaper {
        match super::wallpaper::play(&entry.path) {
            Ok(()) => result["now_playing"] = json!(true),
            Err(why) => result["could_not_play"] = json!(why),
        }
    }
    Ok(result)
}

fn import_file(args: &Value) -> Result<Value, String> {
    let path = std::path::PathBuf::from(args::need_text(args, "path")?);
    if !path.is_file() {
        return Err(format!("There is no file at {}. Give the full path.", path.display()));
    }
    let extension = path.extension().map(|extension| extension.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !IMPORTABLE.contains(&extension.as_str()) {
        return Err(format!("Paperloom can't import a .{extension} file. It takes: {}.", IMPORTABLE.join(", ")));
    }
    add_file(
        &path,
        &After { name: args::text(args, "name"), collection: args::text(args, "collection"), set_as_wallpaper: args::flag(args, "set_as_wallpaper").unwrap_or(false), source_id: None, origin: None },
    )
}

fn import_url(args: &Value) -> Result<Value, String> {
    let url = args::need_text(args, "url")?;
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("The link must start with http:// or https://.".to_string());
    }
    let from_link = url.split(['?', '#']).next().unwrap_or(&url).rsplit('/').find(|part| !part.is_empty()).unwrap_or("wallpaper").to_string();
    let title = args::text(args, "name").unwrap_or(from_link);
    let id = crate::jobs::start(crate::jobs::Spec {
        source: "link".to_string(),
        url: url.clone(),
        title: title.clone(),
        after: After {
            name: Some(title),
            collection: args::text(args, "collection"),
            set_as_wallpaper: args::flag(args, "set_as_wallpaper").unwrap_or(false),
            source_id: Some(format!("link:{url}")),
            origin: None,
        },
    });
    Ok(crate::jobs::wait(id, args::whole(args, "wait_seconds").unwrap_or(40).min(110)))
}

fn remove(args: &Value) -> Result<Value, String> {
    let mut library = Library::load_manifest();
    let entry = find(&library, &args::need_text(args, "wallpaper")?)?;
    let (id, name, kept) = (entry.id, entry.display_name.clone(), entry.mode == ImportMode::ReferenceInPlace);
    if args::flag(args, "confirm") != Some(true) {
        return Err(format!("This removes “{name}” (id {id}) from the library for good. Call again with confirm: true to do it."));
    }
    library.remove(id).map_err(|err| format!("Couldn't remove it: {err}"))?;
    Ok(json!({ "removed": name, "file": if kept { "left where it is (it was only linked)" } else { "deleted" } }))
}

/// Finds the wallpaper, applies `change`, and answers with how it looks now.
fn change(args: &Value, change: impl FnOnce(&mut Library, u64) -> std::io::Result<()>) -> Result<Value, String> {
    let mut library = Library::load_manifest();
    let id = find(&library, &args::need_text(args, "wallpaper")?)?.id;
    change(&mut library, id).map_err(|err| format!("Couldn't save the library: {err}"))?;
    let entry = library.entry(id).ok_or("The wallpaper vanished from the library.")?;
    Ok(describe(&library, entry))
}

fn favorite(args: &Value) -> Result<Value, String> {
    let value = args::flag(args, "favorite").ok_or("Missing “favorite” (true or false).")?;
    change(args, |library, id| library.set_favorite(id, value))
}

fn tags(args: &Value) -> Result<Value, String> {
    let value = args::list(args, "tags").ok_or("Missing “tags” (a list).")?.join(",");
    change(args, |library, id| library.set_tags(id, &value))
}

fn hidden(args: &Value) -> Result<Value, String> {
    let value = args::flag(args, "hidden").ok_or("Missing “hidden” (true or false).")?;
    change(args, |library, id| library.set_hidden(id, value))
}
