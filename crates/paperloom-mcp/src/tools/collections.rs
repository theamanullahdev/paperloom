//! Collections: named groups of wallpapers (links, never copies).

use paperloom_common::Library;
use serde_json::{json, Value};

use super::library::{find, find_collection};
use super::{Kind, Tool};
use crate::args;

const COLLECTION: (&str, &str, &str, bool) = ("collection", "string", "The collection's id or name.", true);
const WALLPAPER: (&str, &str, &str, bool) = ("wallpaper", "string", "The wallpaper's id or (part of) its name.", true);

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "list_collections",
            about: "Every collection with how many wallpapers it holds.",
            kind: Kind::Read,
            fields: &[("include_hidden", "boolean", "Also list hidden collections (left out by default).", false)],
            run: list,
        },
        Tool { name: "create_collection", about: "Make a new, empty collection.", kind: Kind::Change, fields: &[("name", "string", "Its name.", true)], run: create },
        Tool { name: "rename_collection", about: "Give a collection a new name.", kind: Kind::Change, fields: &[COLLECTION, ("name", "string", "The new name.", true)], run: rename },
        Tool {
            name: "delete_collection",
            about: "Delete a collection. Its wallpapers stay in the library. Cannot be undone.",
            kind: Kind::Remove,
            fields: &[COLLECTION, ("confirm", "boolean", "Must be true.", true)],
            run: delete,
        },
        Tool { name: "add_to_collection", about: "Put a wallpaper in a collection.", kind: Kind::Change, fields: &[COLLECTION, WALLPAPER], run: |args| member(args, true) },
        Tool { name: "remove_from_collection", about: "Take a wallpaper out of a collection (it stays in the library).", kind: Kind::Change, fields: &[COLLECTION, WALLPAPER], run: |args| member(args, false) },
    ]
}

fn list(args: &Value) -> Result<Value, String> {
    let library = Library::load_manifest();
    let include_hidden = args::flag(args, "include_hidden").unwrap_or(false);
    let collections: Vec<Value> = library
        .collections
        .iter()
        .filter(|collection| include_hidden || !collection.hidden)
        .map(|collection| json!({ "id": collection.id, "name": collection.name, "wallpapers": collection.members.len(), "hidden": collection.hidden }))
        .collect();
    Ok(json!({ "collections": collections }))
}

fn create(args: &Value) -> Result<Value, String> {
    let name = args::need_text(args, "name")?;
    let mut library = Library::load_manifest();
    if let Ok(existing) = find_collection(&library, &name) {
        return Ok(json!({ "id": existing.id, "name": existing.name, "already_existed": true }));
    }
    let id = library.create_collection(&name).map_err(|err| format!("Couldn't save the library: {err}"))?;
    Ok(json!({ "id": id, "name": name, "already_existed": false }))
}

fn rename(args: &Value) -> Result<Value, String> {
    let mut library = Library::load_manifest();
    let id = find_collection(&library, &args::need_text(args, "collection")?)?.id;
    let name = args::need_text(args, "name")?;
    library.rename_collection(id, &name).map_err(|err| format!("Couldn't save the library: {err}"))?;
    Ok(json!({ "id": id, "name": name }))
}

fn delete(args: &Value) -> Result<Value, String> {
    let mut library = Library::load_manifest();
    let collection = find_collection(&library, &args::need_text(args, "collection")?)?;
    let (id, name, count) = (collection.id, collection.name.clone(), collection.members.len());
    if args::flag(args, "confirm") != Some(true) {
        return Err(format!("This deletes the collection “{name}” ({count} wallpapers stay in the library). Call again with confirm: true to do it."));
    }
    library.delete_collection(id).map_err(|err| format!("Couldn't save the library: {err}"))?;
    Ok(json!({ "deleted": name }))
}

fn member(args: &Value, add: bool) -> Result<Value, String> {
    let mut library = Library::load_manifest();
    let collection = find_collection(&library, &args::need_text(args, "collection")?)?;
    let (collection_id, collection_name) = (collection.id, collection.name.clone());
    let entry = find(&library, &args::need_text(args, "wallpaper")?)?;
    let (entry_id, entry_name) = (entry.id, entry.display_name.clone());
    library.set_in_collection(collection_id, entry_id, add).map_err(|err| format!("Couldn't save the library: {err}"))?;
    Ok(json!({ "collection": collection_name, "wallpaper": entry_name, "in_collection": add }))
}
