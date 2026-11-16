//! The Model Context Protocol itself: JSON-RPC 2.0 messages in, answers
//! out. The same handler serves stdin/stdout and HTTP.

use serde_json::{json, Value};

use crate::tools::{self, Gate, Kind};

/// Protocol revisions this server speaks, newest first. A client asking
/// for one of them gets it back; any other gets the newest.
const VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "Paperloom is the user's live-wallpaper app. These tools do what its window does: see and set the wallpaper, manage the library and collections, search and download wallpapers, place desktop widgets, change rotation and settings. To add a wallpaper from a link, try import_url; if it can't get the video, download the file yourself and call import_file. Call status first to see what is going on.";

/// The tool list in MCP's shape.
pub fn tool_list() -> Value {
    let tools: Vec<Value> = tools::all()
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.about,
                "inputSchema": crate::args::schema(tool.fields),
                "annotations": { "readOnlyHint": tool.kind == Kind::Read, "destructiveHint": tool.kind == Kind::Remove, "openWorldHint": false },
            })
        })
        .collect();
    json!({ "tools": tools })
}

/// One tool call as an MCP result: the answer as JSON text, plus the same
/// thing structured; a failure is a result with `isError`, so the agent
/// reads the reason instead of a protocol error.
fn tool_result(params: &Value, gate: Gate) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
    let arguments = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
    match tools::call(name, &arguments, gate) {
        Ok(value) => json!({
            "content": [{ "type": "text", "text": serde_json::to_string_pretty(&value).unwrap_or_default() }],
            "structuredContent": if value.is_object() { value } else { json!({ "result": value }) },
            "isError": false,
        }),
        Err(message) => json!({ "content": [{ "type": "text", "text": message }], "isError": true }),
    }
}

/// Answers one message. `None` for notifications (nothing is sent back).
pub fn handle(message: &Value, gate: Gate) -> Option<Value> {
    let id = message.get("id").cloned();
    let method = message.get("method").and_then(Value::as_str).unwrap_or_default();
    let params = message.get("params").cloned().unwrap_or(Value::Null);

    let result = match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or_default();
            Ok(json!({
                "protocolVersion": if VERSIONS.contains(&asked) { asked } else { VERSIONS[0] },
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "paperloom", "title": "Paperloom", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tool_list()),
        "tools/call" => Ok(tool_result(&params, gate)),
        // Not offered, but asked for by some clients anyway: an empty list beats an error.
        "resources/list" => Ok(json!({ "resources": [] })),
        "resources/templates/list" => Ok(json!({ "resourceTemplates": [] })),
        "prompts/list" => Ok(json!({ "prompts": [] })),
        _ => Err(json!({ "code": -32601, "message": format!("Method not found: {method}") })),
    };

    // No id = a notification (`notifications/initialized`, …): never answered.
    let id = id?;
    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error }),
    })
}

/// A whole request body: one message or a batch. `None` when nothing needs
/// answering.
pub fn handle_text(body: &str, gate: Gate) -> Option<Value> {
    match serde_json::from_str::<Value>(body) {
        Ok(Value::Array(messages)) => {
            let answers: Vec<Value> = messages.iter().filter_map(|message| handle(message, gate)).collect();
            (!answers.is_empty()).then_some(Value::Array(answers))
        }
        Ok(message) => handle(&message, gate),
        Err(err) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("Parse error: {err}") } })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_echoes_a_known_version() {
        let answer = handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2024-11-05" } }), Gate::Serving).unwrap();
        assert_eq!(answer["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(answer["result"]["serverInfo"]["name"], "paperloom");
        let newest = handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "1999-01-01" } }), Gate::Serving).unwrap();
        assert_eq!(newest["result"]["protocolVersion"], VERSIONS[0]);
    }

    #[test]
    fn notifications_get_no_answer() {
        assert!(handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }), Gate::Serving).is_none());
    }

    #[test]
    fn lists_tools_with_schemas() {
        let answer = handle(&json!({ "jsonrpc": "2.0", "id": "a", "method": "tools/list" }), Gate::Serving).unwrap();
        let tools = answer["result"]["tools"].as_array().unwrap();
        assert!(tools.len() > 20);
        assert!(tools.iter().all(|tool| tool["inputSchema"]["type"] == "object"));
        assert!(tools.iter().any(|tool| tool["name"] == "import_file"));
    }

    #[test]
    fn unknown_methods_and_bad_json_are_errors() {
        let answer = handle(&json!({ "jsonrpc": "2.0", "id": 2, "method": "nope" }), Gate::Serving).unwrap();
        assert_eq!(answer["error"]["code"], -32601);
        assert_eq!(handle_text("{not json", Gate::Serving).unwrap()["error"]["code"], -32700);
    }

    #[test]
    fn an_unknown_tool_is_an_error_result_not_a_protocol_error() {
        let answer = handle(&json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "no_such_tool" } }), Gate::Serving).unwrap();
        assert_eq!(answer["result"]["isError"], true);
    }
}
