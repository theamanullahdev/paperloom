//! Rotation and settings.

use paperloom_common::rotation::Rotation;
use paperloom_common::Settings;
use serde_json::{json, Value};

use super::{Kind, Tool};
use crate::args;

/// Settings an agent never changes: the Hidden password, the sources (they
/// hold API keys), developer mode and where the library lives on disk.
const LOCKED: [&str; 4] = ["hidden_lock", "sources", "developer_mode", "library_folder"];
// (The app theme has its own tools: apply_theme and reset_theme.)

pub fn tools() -> Vec<Tool> {
    vec![
        Tool { name: "get_rotation", about: "How the wallpaper changes by itself: the always-on collection rotation and the weekly schedule rules.", kind: Kind::Read, fields: &[], run: get_rotation },
        Tool {
            name: "set_rotation",
            about: "Change the rotation. Give only what should change, with get_rotation's field names (enabled, collection = a collection id, interval_minutes, shuffle, rules).",
            kind: Kind::Change,
            fields: &[("changes", "object", "For example {\"enabled\": true, \"collection\": 3, \"interval_minutes\": 30, \"shuffle\": true}.", true)],
            run: set_rotation,
        },
        Tool { name: "get_settings", about: "Paperloom's settings (API keys and the Hidden password are left out).", kind: Kind::Read, fields: &[], run: get_settings },
        Tool {
            name: "set_setting",
            about: "Change one setting, by its name in get_settings. A nested one is written with a dot: updates.check_on_startup.",
            kind: Kind::Settings,
            fields: &[("key", "string", "The setting's name, e.g. pause_on_low_battery or theme.", true), ("value", "any", "The new value, in the same form get_settings shows.", true)],
            run: set_setting,
        },
    ]
}

/// A form sends an object as text; take either.
fn object(args: &Value, key: &str) -> Result<Value, String> {
    match args.get(key) {
        Some(Value::String(text)) => serde_json::from_str(text).map_err(|err| format!("“{key}” isn't valid JSON: {err}")),
        Some(value @ Value::Object(_)) => Ok(value.clone()),
        _ => Err(format!("Missing “{key}” (an object).")),
    }
}

fn get_rotation(_args: &Value) -> Result<Value, String> {
    let mut rotation = serde_json::to_value(Rotation::load()).map_err(|err| err.to_string())?;
    let library = paperloom_common::Library::load_manifest();
    rotation["collections"] = json!(library.collections.iter().map(|collection| json!({ "id": collection.id, "name": collection.name })).collect::<Vec<_>>());
    rotation["rule_fields"] = json!("start/end = minutes after midnight (local time); days = bits, 1 = Monday … 64 = Sunday (127 = every day)");
    Ok(rotation)
}

fn set_rotation(args: &Value) -> Result<Value, String> {
    let changes = object(args, "changes")?;
    let mut current = serde_json::to_value(Rotation::load()).map_err(|err| err.to_string())?;
    args::merge(&mut current, &changes, "")?;
    let rotation: Rotation = serde_json::from_value(current).map_err(|err| format!("That isn't a valid rotation: {err}"))?;
    if let Some(id) = rotation.collection {
        if paperloom_common::Library::load_manifest().collection(id).is_none() {
            return Err(format!("There is no collection with id {id}. Use list_collections."));
        }
    }
    rotation.save().map_err(|err| format!("Couldn't save the rotation: {err}"))?;
    get_rotation(&Value::Null)
}

/// The settings with nothing secret in them.
fn public(settings: &Settings) -> Value {
    let mut value = serde_json::to_value(settings).unwrap_or_default();
    if let Some(object) = value.as_object_mut() {
        object.insert("hidden_lock".to_string(), json!(if settings.hidden_lock.is_some() { "set" } else { "not set" }));
    }
    if let Some(sources) = value.get_mut("sources").and_then(Value::as_array_mut) {
        for source in sources {
            if let Some(key) = source.get_mut("api_key") {
                *key = json!(if key.as_str().is_some_and(|key| !key.is_empty()) { "set" } else { "" });
            }
        }
    }
    value
}

fn get_settings(_args: &Value) -> Result<Value, String> {
    Ok(public(&paperloom_common::load()))
}

fn set_setting(args: &Value) -> Result<Value, String> {
    let key = args::need_text(args, "key")?;
    let mut value = args.get("value").cloned().ok_or("Missing “value”.")?;
    if LOCKED.contains(&key.split('.').next().unwrap_or_default()) {
        return Err(format!("“{key}” can only be changed in Paperloom's own window."));
    }
    let settings = paperloom_common::load();
    let mut current = serde_json::to_value(&settings).map_err(|err| err.to_string())?;

    // Walk to the setting; it has to exist already.
    let mut slot = &mut current;
    for part in key.split('.') {
        slot = slot.get_mut(part).ok_or_else(|| format!("There is no setting called “{key}”. Use get_settings to see the names."))?;
    }
    // A form sends everything as text: read "true"/"12" as what the setting is.
    if let (Some(text), false) = (value.as_str(), slot.is_string()) {
        if let Ok(parsed) = serde_json::from_str::<Value>(text) {
            value = parsed;
        }
    }
    *slot = value;
    let changed: Settings = serde_json::from_value(current).map_err(|err| format!("That value doesn't fit “{key}”: {err}"))?;
    paperloom_common::save(&changed).map_err(|err| format!("Couldn't save the settings: {err}"))?;
    let after = public(&changed);
    let mut shown = &after;
    for part in key.split('.') {
        shown = shown.get(part).unwrap_or(&Value::Null);
    }
    Ok(json!({ "key": key, "value": shown }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_never_leave() {
        let mut settings = Settings { hidden_lock: Some("salt$hash".into()), ..Settings::default() }.with_builtin_sources();
        if let Some(source) = settings.sources.first_mut() {
            source.api_key = "super-secret".into();
        }
        let text = public(&settings).to_string();
        assert!(!text.contains("super-secret"));
        assert!(!text.contains("salt$hash"));
    }

    #[test]
    fn locked_settings_are_refused() {
        for key in LOCKED {
            let refused = set_setting(&json!({ "key": key, "value": 1 })).unwrap_err();
            assert!(refused.contains("own window"), "{key}: {refused}");
        }
    }
}
