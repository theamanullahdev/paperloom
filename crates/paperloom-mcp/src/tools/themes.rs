//! App themes: list them, read the format, check and install a theme an
//! agent wrote, switch to one. A theme is a small text file
//! (`paperloom_theme`), so writing one from a description is an agent's job.

use paperloom_theme::{Listed, DEFAULT_ID};
use serde_json::{json, Value};

use super::{Kind, Tool};
use crate::args;

const THEME: (&str, &str, &str, bool) = ("theme", "string", "The theme's id or name (from list_themes).", true);

pub fn tools() -> Vec<Tool> {
    vec![
        Tool { name: "list_themes", about: "The app themes on this computer and which one is in use.", kind: Kind::Read, fields: &[], run: list },
        Tool {
            name: "get_theme_schema",
            about: "How to write a theme: the whole theme.toml format with every field explained. Read this before writing a theme.",
            kind: Kind::Read,
            fields: &[],
            run: |_| Ok(json!({ "format": "TOML", "schema": paperloom_theme::SCHEMA, "how": "Write a theme.toml, check it with validate_theme, add it with install_theme, switch to it with apply_theme." })),
        },
        Tool { name: "get_theme", about: "One theme's file (its theme.toml text) and its folder.", kind: Kind::Read, fields: &[THEME], run: get },
        Tool {
            name: "validate_theme",
            about: "Check a theme.toml without installing it: what makes it unusable (errors) and what is ignored or looks wrong (warnings).",
            kind: Kind::Read,
            fields: &[("toml", "string", "The text of a theme.toml.", true)],
            run: validate,
        },
        Tool {
            name: "install_theme",
            about: "Add a theme to this computer from the text of a theme.toml, or from a theme folder / theme.toml / .paperloomtheme file. Installing a theme with an id that is already there replaces it.",
            kind: Kind::Change,
            fields: &[
                ("toml", "string", "The text of a theme.toml (give this or path).", false),
                ("path", "string", "Full path of a theme folder, a theme.toml or a .paperloomtheme file.", false),
                ("apply", "boolean", "Also switch to it.", false),
            ],
            run: install,
        },
        Tool { name: "apply_theme", about: "Switch Paperloom's window to a theme.", kind: Kind::Change, fields: &[THEME], run: apply },
        Tool { name: "reset_theme", about: "Switch back to Paperloom's own look.", kind: Kind::Change, fields: &[], run: |_| set(DEFAULT_ID) },
        Tool {
            name: "remove_theme",
            about: "Delete an installed theme from this computer (built-in ones can't be removed). Cannot be undone.",
            kind: Kind::Remove,
            fields: &[THEME, ("confirm", "boolean", "Must be true.", true)],
            run: remove,
        },
    ]
}

fn in_use() -> String {
    let saved = paperloom_common::load().app_theme;
    if saved.is_empty() {
        DEFAULT_ID.to_string()
    } else {
        saved
    }
}

/// The theme `wanted` names: its id, or its name ignoring case.
fn find(wanted: &str) -> Result<Listed, String> {
    let all = paperloom_theme::list(&paperloom_common::themes_dir());
    let wanted = wanted.trim();
    all.iter()
        .find(|listed| listed.theme.id == wanted)
        .or_else(|| all.iter().find(|listed| listed.theme.name.eq_ignore_ascii_case(wanted) || listed.theme.id.eq_ignore_ascii_case(wanted)))
        .cloned()
        .ok_or_else(|| format!("There is no theme called “{wanted}”. Themes: {}.", all.iter().map(|listed| listed.theme.id.clone()).collect::<Vec<_>>().join(", ")))
}

fn describe(listed: &Listed, current: &str) -> Value {
    let theme = &listed.theme;
    json!({
        "id": theme.id,
        "name": theme.name,
        "author": theme.author,
        "description": theme.description,
        "built_in": listed.built_in(),
        "in_use": theme.id == current,
        "navigation": theme.layout.nav.to_string().to_lowercase(),
        "folder": listed.dir(),
    })
}

fn list(_args: &Value) -> Result<Value, String> {
    let current = in_use();
    let themes: Vec<Value> = paperloom_theme::list(&paperloom_common::themes_dir()).iter().map(|listed| describe(listed, &current)).collect();
    Ok(json!({ "in_use": current, "themes": themes, "themes_folder": paperloom_common::themes_dir() }))
}

fn get(args: &Value) -> Result<Value, String> {
    let listed = find(&args::need_text(args, "theme")?)?;
    // The file as written when there is one (comments and all).
    let text = listed.dir().and_then(|dir| std::fs::read_to_string(dir.join("theme.toml")).ok()).unwrap_or_else(|| toml::to_string_pretty(&listed.theme).unwrap_or_default());
    let mut details = describe(&listed, &in_use());
    details["toml"] = json!(text);
    Ok(details)
}

fn validate(args: &Value) -> Result<Value, String> {
    let report = paperloom_theme::check(&args::need_text(args, "toml")?, None);
    Ok(json!({ "usable": report.usable(), "errors": report.errors, "warnings": report.warnings }))
}

fn install(args: &Value) -> Result<Value, String> {
    let themes_dir = paperloom_common::themes_dir();
    let imported = match (args::text(args, "toml"), args::text(args, "path")) {
        (Some(text), _) => paperloom_fetch::themes::import_text(&text, &themes_dir),
        (None, Some(path)) => paperloom_fetch::themes::import_path(std::path::Path::new(&path), &themes_dir),
        (None, None) => Err("Give “toml” (the theme's text) or “path”.".to_string()),
    }?;
    let mut result = json!({ "installed": imported.id, "name": imported.name, "warnings": imported.warnings, "folder": themes_dir.join(paperloom_theme::folder_name(&imported.id)) });
    if args::flag(args, "apply") == Some(true) {
        set(&imported.id)?;
        result["in_use"] = json!(true);
    }
    Ok(result)
}

/// Makes `id` the theme in use. An open window follows within two seconds;
/// otherwise it shows at the next start.
fn set(id: &str) -> Result<Value, String> {
    let mut settings = paperloom_common::load();
    settings.app_theme = if id == DEFAULT_ID { String::new() } else { id.to_string() };
    paperloom_common::save(&settings).map_err(|err| format!("Couldn't save the choice: {err}"))?;
    Ok(json!({ "in_use": id, "note": "An open Paperloom window switches within a couple of seconds. A theme's text font shows after Paperloom restarts." }))
}

fn apply(args: &Value) -> Result<Value, String> {
    set(&find(&args::need_text(args, "theme")?)?.theme.id)
}

fn remove(args: &Value) -> Result<Value, String> {
    let listed = find(&args::need_text(args, "theme")?)?;
    if listed.built_in() {
        return Err(format!("“{}” ships with Paperloom and can't be removed.", listed.theme.name));
    }
    if args::flag(args, "confirm") != Some(true) {
        return Err(format!("This deletes the theme “{}” from this computer. Call again with confirm: true to do it.", listed.theme.name));
    }
    let id = listed.theme.id.clone();
    if in_use() == id {
        set(DEFAULT_ID)?;
    }
    paperloom_theme::remove(&paperloom_common::themes_dir(), &id).map_err(|err| format!("Couldn't remove it: {err}"))?;
    Ok(json!({ "removed": id }))
}
