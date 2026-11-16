//! Every tool an agent can call, and the one door they all go through
//! (`call`): the on/off switch, the permission switches, and the log.

mod collections;
pub mod library;
mod online;
mod system;
mod themes;
mod wallpaper;
mod widgets;

use paperloom_common::mcp::McpConfig;
use serde_json::Value;

/// What a tool does to the user's things, for the permission switches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Only looks.
    Read,
    /// Changes something ("Allow changes").
    Change,
    /// Removes something for good ("Allow removing").
    Remove,
    /// Changes Paperloom's settings ("Allow changing settings").
    Settings,
}

pub struct Tool {
    pub name: &'static str,
    pub about: &'static str,
    pub kind: Kind,
    /// `(name, type, what it is, required)`; types: string, integer,
    /// number, boolean, object, list, any.
    pub fields: &'static [(&'static str, &'static str, &'static str, bool)],
    pub run: fn(&Value) -> Result<Value, String>,
}

pub fn all() -> Vec<Tool> {
    let mut tools = Vec::new();
    tools.extend(wallpaper::tools());
    tools.extend(library::tools());
    tools.extend(collections::tools());
    tools.extend(online::tools());
    tools.extend(widgets::tools());
    tools.extend(themes::tools());
    tools.extend(system::tools());
    tools
}

/// Who is asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// The running server itself: MCP is on by definition.
    Serving,
    /// Another way in (stdin/stdout, `paperloom-mcp call`): only while the
    /// user has MCP switched on.
    NeedsSwitch,
}

pub const OFF_MESSAGE: &str = "MCP is switched off in Paperloom. Ask the user to open Paperloom, go to the MCP page and switch it on, then try again.";

/// Runs one tool by name. Errors are sentences an agent can act on.
pub fn call(name: &str, args: &Value, gate: Gate) -> Result<Value, String> {
    if gate == Gate::NeedsSwitch && !paperloom_common::mcp::is_running() {
        return Err(OFF_MESSAGE.to_string());
    }
    let tools = all();
    let Some(tool) = tools.iter().find(|tool| tool.name == name) else {
        return Err(format!("There is no tool called “{name}”. Tools: {}.", tools.iter().map(|tool| tool.name).collect::<Vec<_>>().join(", ")));
    };
    let empty = Value::Object(Default::default());
    let result = allowed(tool.kind, &McpConfig::load()).and_then(|()| (tool.run)(if args.is_null() { &empty } else { args }));
    match &result {
        Ok(_) => paperloom_common::log::info(&format!("{name}: done")),
        Err(why) => paperloom_common::log::warn(&format!("{name}: {why}")),
    }
    result
}

fn allowed(kind: Kind, config: &McpConfig) -> Result<(), String> {
    let refuse = |switch: &str| Err(format!("Not allowed: “{switch}” is switched off on Paperloom's MCP page. Ask the user to switch it on if they want this."));
    match kind {
        Kind::Read => Ok(()),
        _ if !config.allow_changes => refuse("Allow changes"),
        Kind::Change => Ok(()),
        Kind::Remove if !config.allow_remove => refuse("Allow removing"),
        Kind::Settings if !config.allow_settings => refuse("Allow changing settings"),
        Kind::Remove | Kind::Settings => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_schemas_are_objects() {
        let tools = all();
        let mut names: Vec<_> = tools.iter().map(|tool| tool.name).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "two tools share a name");
        for tool in &tools {
            assert_eq!(crate::args::schema(tool.fields)["type"], "object", "{}", tool.name);
            assert!(!tool.about.is_empty());
        }
    }

    #[test]
    fn switches_gate_each_kind() {
        let mut config = McpConfig::default();
        assert!(allowed(Kind::Read, &config).is_ok());
        assert!(allowed(Kind::Change, &config).is_ok());
        assert!(allowed(Kind::Remove, &config).is_err());
        assert!(allowed(Kind::Settings, &config).is_err());
        config.allow_remove = true;
        config.allow_settings = true;
        assert!(allowed(Kind::Remove, &config).is_ok());
        assert!(allowed(Kind::Settings, &config).is_ok());
        config.allow_changes = false;
        assert!(allowed(Kind::Read, &config).is_ok());
        assert!(allowed(Kind::Change, &config).is_err());
        assert!(allowed(Kind::Remove, &config).is_err());
    }
}
