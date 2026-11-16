//! The text a user copies to an agent: where Paperloom is and every way to
//! talk to it. The same text is served at `/guide`.

use paperloom_common::mcp::McpConfig;

pub fn text(config: &McpConfig) -> String {
    let base = config.base_url();
    let token = &config.token;
    let program = paperloom_common::process::current_exe().map(|path| path.display().to_string()).unwrap_or_else(|| "paperloom-mcp".to_string());
    let mut tools = String::new();
    for tool in crate::tools::all() {
        let fields: Vec<String> = tool.fields.iter().map(|(name, _, _, needed)| if *needed { (*name).to_string() } else { format!("{name}?") }).collect();
        tools.push_str(&format!("- {}({}): {}\n", tool.name, fields.join(", "), tool.about));
    }
    format!(
        r#"You can control Paperloom, the live-wallpaper app on this computer. It is listening on this computer only.

Address: {base}
Token:   {token}

Use whichever of these you can:

1. MCP (HTTP): connect to {base}/mcp with the header "Authorization: Bearer {token}".
   If you can't set headers, use {base}/mcp?token={token}

2. MCP (a program you start, stdin/stdout): run  "{program}"

3. Plain HTTP + JSON:
   list the tools:  curl -s "{base}/tools?token={token}"
   call a tool:     curl -s -X POST "{base}/call/status?token={token}"
                    curl -s -X POST "{base}/call/import_file?token={token}" -H "Content-Type: application/json" -d '{{"path":"/full/path/video.mp4"}}'
   The answer is {{"ok":true,"result":…}} or {{"ok":false,"error":"why"}}.

4. A web browser: open {base}/?token={token}
   Every tool is a form on that page; submitting one shows its result.

5. A shell:  "{program}" call status
             "{program}" call import_file '{{"path":"/full/path/video.mp4"}}'

To add a wallpaper from a link: call import_url. If it answers that it couldn't get a video, download the file yourself and call import_file with its path. Start with status.

Tools (a ? marks an optional argument):
{tools}"#
    )
}
