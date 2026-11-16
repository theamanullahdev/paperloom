//! Desktop widgets: what is placed, add, move and restyle, remove, on/off.

use paperloom_common::widgets::{self, Widgets, BUILT_IN};
use serde_json::{json, Value};

use super::{Kind, Tool};
use crate::args;

pub fn tools() -> Vec<Tool> {
    vec![
        Tool { name: "list_widgets", about: "The widgets on the desktop, whether widgets are on, and the kinds that can be added.", kind: Kind::Read, fields: &[], run: list },
        Tool { name: "set_widgets_on", about: "Show or hide all desktop widgets.", kind: Kind::Change, fields: &[("on", "boolean", "true = show them.", true)], run: set_on },
        Tool {
            name: "add_widget",
            about: "Place a new widget on the desktop.",
            kind: Kind::Change,
            fields: &[
                ("kind", "string", "clock, date, cpu, memory, disk, network, battery, uptime, weather or nowplaying.", true),
                ("x", "number", "Left edge as a fraction of the screen width, 0 to 0.95.", false),
                ("y", "number", "Top edge as a fraction of the screen height, 0 to 0.95.", false),
                ("skin", "string", "The look that draws it, e.g. “clock/neon”. Empty = the kind's default.", false),
            ],
            run: add,
        },
        Tool {
            name: "update_widget",
            about: "Move, resize or restyle one widget. Give only what should change.",
            kind: Kind::Change,
            fields: &[
                ("id", "integer", "The widget's id (from list_widgets).", true),
                ("x", "number", "Left edge, 0 to 0.95 of the screen width.", false),
                ("y", "number", "Top edge, 0 to 0.95 of the screen height.", false),
                ("scale", "number", "Size, 0.5 to 3 (1 = natural size).", false),
                ("opacity", "number", "0.1 to 1.", false),
                ("skin", "string", "The look that draws it, e.g. “clock/neon”.", false),
                ("drag", "boolean", "It can be dragged on the desktop.", false),
                ("menu", "boolean", "Right-clicking it opens its menu.", false),
            ],
            run: update,
        },
        Tool { name: "remove_widget", about: "Take a widget off the desktop.", kind: Kind::Change, fields: &[("id", "integer", "The widget's id (from list_widgets).", true)], run: remove },
    ]
}

fn list(_args: &Value) -> Result<Value, String> {
    let widgets = Widgets::load();
    Ok(json!({
        "on": widgets.enabled,
        "running": widgets::is_running(),
        "placed": serde_json::to_value(&widgets.placed).unwrap_or_default(),
        "weather_city": widgets.weather_city,
        "kinds": BUILT_IN.iter().map(|entry| json!({ "kind": entry.kind, "name": entry.name, "about": entry.about })).collect::<Vec<_>>(),
    }))
}

fn set_on(args: &Value) -> Result<Value, String> {
    let on = args::flag(args, "on").ok_or("Missing “on” (true or false).")?;
    widgets::set_enabled(on).map_err(|err| format!("Couldn't switch widgets: {err}"))?;
    Ok(json!({ "on": on }))
}

/// Applies the optional placement fields of `args` to widget `id`.
fn apply(widgets: &mut Widgets, id: u32, args: &Value) -> Result<(), String> {
    let placed = widgets.placed.iter_mut().find(|placed| placed.id == id).ok_or_else(|| format!("There is no widget with id {id}. Use list_widgets."))?;
    if let Some(x) = args::number(args, "x") {
        placed.x = (x as f32).clamp(0.0, 0.95);
    }
    if let Some(y) = args::number(args, "y") {
        placed.y = (y as f32).clamp(0.0, 0.95);
    }
    if let Some(scale) = args::number(args, "scale") {
        placed.scale = (scale as f32).clamp(0.5, 3.0);
    }
    if let Some(opacity) = args::number(args, "opacity") {
        placed.opacity = (opacity as f32).clamp(0.1, 1.0);
    }
    if let Some(skin) = args.get("skin").and_then(Value::as_str) {
        placed.skin = skin.trim().to_string();
    }
    if let Some(drag) = args::flag(args, "drag") {
        placed.drag = drag;
    }
    if let Some(menu) = args::flag(args, "menu") {
        placed.menu = menu;
    }
    Ok(())
}

fn saved(widgets: &Widgets, id: u32) -> Result<Value, String> {
    widgets.save().map_err(|err| format!("Couldn't save the widgets: {err}"))?;
    // The widgets process follows the file by itself; start it if it should be showing.
    if widgets.enabled {
        let _ = widgets::start();
    }
    Ok(json!({ "widget": widgets.placed.iter().find(|placed| placed.id == id), "widgets_on": widgets.enabled }))
}

fn add(args: &Value) -> Result<Value, String> {
    let kind = args::need_text(args, "kind")?.to_lowercase();
    let mut widgets = Widgets::load();
    let id = widgets
        .add(&kind)
        .ok_or_else(|| format!("Couldn't add “{kind}”: kinds are {}, and at most {} widgets fit.", BUILT_IN.iter().map(|entry| entry.kind).collect::<Vec<_>>().join(", "), widgets::MAX_PLACED))?;
    apply(&mut widgets, id, args)?;
    saved(&widgets, id)
}

fn update(args: &Value) -> Result<Value, String> {
    let id = args::whole(args, "id").ok_or("Missing “id”.")? as u32;
    let mut widgets = Widgets::load();
    apply(&mut widgets, id, args)?;
    saved(&widgets, id)
}

fn remove(args: &Value) -> Result<Value, String> {
    let id = args::whole(args, "id").ok_or("Missing “id”.")? as u32;
    let mut widgets = Widgets::load();
    if !widgets.placed.iter().any(|placed| placed.id == id) {
        return Err(format!("There is no widget with id {id}. Use list_widgets."));
    }
    widgets.remove(id);
    widgets.save().map_err(|err| format!("Couldn't save the widgets: {err}"))?;
    Ok(json!({ "removed": id }))
}
