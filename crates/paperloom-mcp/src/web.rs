//! The web page for agents that drive a browser: every tool is a plain
//! form (labelled inputs, one button), and submitting one opens a page
//! with the result. No scripts, so any browser agent can read and use it.

use paperloom_common::mcp::McpConfig;
use serde_json::Value;

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

const STYLE: &str = "body{font:15px/1.5 system-ui,sans-serif;background:#1c1917;color:#f5efe9;margin:0;padding:24px;max-width:860px;margin:auto}\
h1{font-size:22px;margin:0 0 4px}h2{font-size:16px;margin:0}p{margin:6px 0;color:#a8998e}a{color:#fb923c}\
form{background:#262220;border:1px solid #3a332e;border-radius:6px;padding:14px 16px;margin:12px 0}\
label{display:block;margin:10px 0 2px}small{color:#a8998e}\
input[type=text]{width:100%;box-sizing:border-box;padding:7px 9px;background:#1c1917;color:#f5efe9;border:1px solid #3a332e;border-radius:4px}\
button{margin-top:12px;padding:7px 14px;background:#fb923c;color:#1c1917;border:0;border-radius:4px;font-weight:600;cursor:pointer}\
pre{background:#262220;border:1px solid #3a332e;border-radius:6px;padding:14px;overflow:auto;white-space:pre-wrap}\
.bad{border-color:#f87171;color:#f87171}";

fn page(title: &str, body: &str) -> String {
    format!("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><style>{STYLE}</style></head><body>{body}</body></html>", escape(title))
}

/// Shown when the link has no (or an old) token.
pub fn locked() -> String {
    page("Paperloom", "<h1>Paperloom</h1><p>This page needs the link from Paperloom's MCP page (it carries a token). Open Paperloom, go to MCP and copy the browser link again.</p>")
}

pub fn home(config: &McpConfig) -> String {
    let token = escape(&config.token);
    let mut body = format!(
        "<h1>Paperloom</h1><p>Control the live-wallpaper app on this computer. Each box below is one action: fill in what it asks for and press its button; the next page shows the result. Start with <a href=\"/run/status?token={token}\">status</a>. Written instructions: <a href=\"/guide?token={token}\">guide</a>.</p>"
    );
    for tool in crate::tools::all() {
        body.push_str(&format!("<form method=\"get\" action=\"/run/{name}\" id=\"{name}\"><h2>{name}</h2><p>{about}</p><input type=\"hidden\" name=\"token\" value=\"{token}\">", name = tool.name, about = escape(tool.about)));
        for (field, kind, about, needed) in tool.fields {
            let hint = match *kind {
                "boolean" => " (true or false)",
                "list" => " (separate with commas)",
                "object" | "any" => " (JSON)",
                "integer" | "number" => " (a number)",
                _ => "",
            };
            body.push_str(&format!(
                "<label for=\"{tool}-{field}\">{field}{star}</label><small>{about}{hint}</small><input type=\"text\" id=\"{tool}-{field}\" name=\"{field}\"{required}>",
                tool = tool.name,
                star = if *needed { " (required)" } else { "" },
                about = escape(about),
                required = if *needed { " required" } else { "" },
            ));
        }
        body.push_str(&format!("<button type=\"submit\">Run {}</button></form>", tool.name));
    }
    page("Paperloom", &body)
}

pub fn result(config: &McpConfig, tool: &str, outcome: &Result<Value, String>) -> String {
    let token = escape(&config.token);
    let (heading, class, text) = match outcome {
        Ok(value) => ("Done", "", serde_json::to_string_pretty(value).unwrap_or_default()),
        Err(why) => ("It didn't work", " class=\"bad\"", why.clone()),
    };
    page(
        &format!("{tool}: {heading}"),
        &format!("<h1>{}: {heading}</h1><pre id=\"result\"{class}>{}</pre><p><a href=\"/?token={token}\">Back to all actions</a></p>", escape(tool), escape(&text)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_has_a_form_for_every_tool_and_escapes_text() {
        let config = McpConfig { token: "abc123abc123abc123".into(), ..McpConfig::default() };
        let html = home(&config);
        for tool in crate::tools::all() {
            assert!(html.contains(&format!("action=\"/run/{}\"", tool.name)), "{}", tool.name);
        }
        let shown = result(&config, "status", &Err("<script>x</script>".into()));
        assert!(shown.contains("&lt;script&gt;") && !shown.contains("<script>"));
    }
}
