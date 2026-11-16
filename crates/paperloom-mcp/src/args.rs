//! Reading a tool's arguments, forgivingly: agents (and the web page's
//! forms) send `"true"` for `true` and `"12"` for `12` often enough.

use serde_json::{json, Map, Value};

pub fn text(args: &Value, key: &str) -> Option<String> {
    match args.get(key)? {
        Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

pub fn need_text(args: &Value, key: &str) -> Result<String, String> {
    text(args, key).ok_or_else(|| format!("Missing “{key}”."))
}

pub fn flag(args: &Value, key: &str) -> Option<bool> {
    match args.get(key)? {
        Value::Bool(value) => Some(*value),
        Value::String(text) => match text.trim().to_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Some(true),
            "false" | "no" | "off" | "0" => Some(false),
            _ => None,
        },
        Value::Number(number) => Some(number.as_f64() != Some(0.0)),
        _ => None,
    }
}

pub fn number(args: &Value, key: &str) -> Option<f64> {
    match args.get(key)? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

pub fn whole(args: &Value, key: &str) -> Option<u64> {
    number(args, key).filter(|value| *value >= 0.0).map(|value| value as u64)
}

/// A list given as a JSON array or as "a, b, c".
pub fn list(args: &Value, key: &str) -> Option<Vec<String>> {
    match args.get(key)? {
        Value::Array(items) => Some(items.iter().filter_map(|item| item.as_str().map(|text| text.trim().to_string())).filter(|text| !text.is_empty()).collect()),
        Value::String(text) => Some(text.split(',').map(|part| part.trim().to_string()).filter(|part| !part.is_empty()).collect()),
        _ => None,
    }
}

/// A tool's input schema from `(name, type, what it is, required)` rows.
pub fn schema(fields: &[(&str, &str, &str, bool)]) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for (name, kind, about, needed) in fields {
        let mut property = match *kind {
            "list" => json!({ "type": "array", "items": { "type": "string" } }),
            "any" => json!({}),
            other => json!({ "type": other }),
        };
        property["description"] = json!(about);
        properties.insert((*name).to_string(), property);
        if *needed {
            required.push(*name);
        }
    }
    json!({ "type": "object", "properties": properties, "required": required })
}

/// Lays `changes` over `current` (objects merge key by key; anything else
/// replaces). A key `current` doesn't have is refused, so a typo is an
/// error instead of a silently ignored field.
pub fn merge(current: &mut Value, changes: &Value, path: &str) -> Result<(), String> {
    match (current, changes) {
        (Value::Object(current), Value::Object(changes)) => {
            for (key, change) in changes {
                let here = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
                match current.get_mut(key) {
                    Some(slot) => merge(slot, change, &here)?,
                    None => return Err(format!("There is no “{here}”. Known: {}.", current.keys().cloned().collect::<Vec<_>>().join(", "))),
                }
            }
            Ok(())
        }
        (slot, change) => {
            *slot = change.clone();
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_loose_values() {
        let args = json!({ "a": "true", "b": 0, "n": "12", "tags": "rain, night ,", "name": "  x " });
        assert_eq!(flag(&args, "a"), Some(true));
        assert_eq!(flag(&args, "b"), Some(false));
        assert_eq!(whole(&args, "n"), Some(12));
        assert_eq!(list(&args, "tags"), Some(vec!["rain".to_string(), "night".to_string()]));
        assert_eq!(text(&args, "name").as_deref(), Some("x"));
        assert!(need_text(&args, "missing").is_err());
    }

    #[test]
    fn merge_refuses_unknown_keys() {
        let mut current = json!({ "enabled": false, "inner": { "x": 1 } });
        merge(&mut current, &json!({ "enabled": true, "inner": { "x": 2 } }), "").unwrap();
        assert_eq!(current, json!({ "enabled": true, "inner": { "x": 2 } }));
        assert!(merge(&mut current, &json!({ "inner": { "y": 2 } }), "").unwrap_err().contains("inner.y"));
    }
}
