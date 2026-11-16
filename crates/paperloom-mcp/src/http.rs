//! The local server (`paperloom-mcp serve`): one small HTTP listener on
//! 127.0.0.1 that speaks MCP (`POST /mcp`), a plain JSON API (`/tools`,
//! `/call/<tool>`) and a web page with a form per tool (`/`, `/run/<tool>`)
//! for agents that drive a browser.
//!
//! Hand-written on `std::net` on purpose: it blocks in `accept()` between
//! requests (no polling, no runtime, no timers), so a switched-on but
//! unused server costs no CPU. Every request must carry the token, and the
//! `Host` must be this computer, so a web page elsewhere can't reach it.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use paperloom_common::mcp::{self, McpConfig};
use serde_json::{json, Value};

use crate::tools::{self, Gate};

/// Switched on and then forgotten: it ends itself after this long without a request.
const IDLE_LIMIT_SECS: u64 = 12 * 60 * 60;
const MAX_BODY: usize = 8 * 1024 * 1024;

static LAST_REQUEST: AtomicU64 = AtomicU64::new(0);

pub fn serve(port: Option<u16>) -> i32 {
    let mut config = McpConfig::load();
    let wanted = port.unwrap_or(config.port);
    let Some((listener, bound)) = bind(wanted) else {
        if mcp::is_running_on(wanted) {
            println!("Paperloom's MCP server is already on (port {wanted}).");
            return 0;
        }
        eprintln!("No free port near {wanted}.");
        return 1;
    };
    if config.port != bound {
        config.port = bound;
        let _ = config.save();
    }
    paperloom_common::log::info(&format!("listening on 127.0.0.1:{bound}"));
    println!("Paperloom MCP server on http://127.0.0.1:{bound}");

    LAST_REQUEST.store(paperloom_common::log::now_unix(), Ordering::Relaxed);
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_secs(600));
        if paperloom_common::log::now_unix().saturating_sub(LAST_REQUEST.load(Ordering::Relaxed)) > IDLE_LIMIT_SECS {
            paperloom_common::log::info("no requests for 12 hours: switching off");
            std::process::exit(0);
        }
    });

    for stream in listener.incoming().flatten() {
        std::thread::spawn(move || {
            let _ = handle(stream);
        });
    }
    0
}

/// The wanted port, or the next free one after it (when something else has it).
fn bind(wanted: u16) -> Option<(TcpListener, u16)> {
    (0..20u16).find_map(|step| {
        let port = wanted.checked_add(step)?;
        // Our own server already there: don't start a second one beside it.
        if step == 0 && mcp::is_running_on(port) {
            return None;
        }
        TcpListener::bind((Ipv4Addr::LOCALHOST, port)).ok().map(|listener| (listener, port))
    })
}

struct Request {
    method: String,
    path: String,
    query: Vec<(String, String)>,
    headers: HashMap<String, String>,
    body: String,
}

impl Request {
    fn param(&self, name: &str) -> Option<&str> {
        self.query.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    fn token(&self) -> &str {
        let bearer = self.headers.get("authorization").and_then(|value| value.strip_prefix("Bearer ").or_else(|| value.strip_prefix("bearer ")));
        bearer.or(self.headers.get("x-paperloom-token").map(String::as_str)).or(self.param("token")).unwrap_or_default().trim()
    }

    /// Only a request addressed to this computer by its local name: refuses a
    /// web page that made a foreign name point at 127.0.0.1.
    fn local_host(&self) -> bool {
        let host = self.headers.get("host").map(String::as_str).unwrap_or_default();
        let name = host.strip_prefix("[::1]").map(|_| "[::1]").unwrap_or_else(|| host.split(':').next().unwrap_or_default());
        matches!(name, "127.0.0.1" | "localhost" | "[::1]")
    }
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => out.push(b' '),
            b'%' if index + 2 < bytes.len() => match std::str::from_utf8(&bytes[index + 1..index + 3]).ok().and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                Some(byte) => {
                    out.push(byte);
                    index += 2;
                }
                None => out.push(b'%'),
            },
            other => out.push(other),
        }
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(key), decode(value))
        })
        .collect()
}

fn read_request(stream: &TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream.try_clone().ok()?.take((MAX_BODY + 64 * 1024) as u64));
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_uppercase();
    let target = parts.next()?.to_string();
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));

    let mut headers = HashMap::new();
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).ok()? == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.insert(name.trim().to_lowercase(), value.trim().to_string());
        }
    }
    let length = headers.get("content-length").and_then(|value| value.parse::<usize>().ok()).unwrap_or(0).min(MAX_BODY);
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).ok()?;
    let mut query = parse_query(query);
    let mut body = String::from_utf8_lossy(&body).into_owned();
    // A form posted from the web page: its fields are the arguments.
    if headers.get("content-type").is_some_and(|kind| kind.starts_with("application/x-www-form-urlencoded")) {
        query.extend(parse_query(&body));
        body.clear();
    }
    Some(Request { method, path: decode(path), query, headers, body })
}

fn respond(mut stream: &TcpStream, status: &str, kind: &str, body: &str) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: Authorization, Content-Type, Accept, X-Paperloom-Token, Mcp-Session-Id, Mcp-Protocol-Version, Last-Event-ID\r\nAccess-Control-Allow-Methods: GET, POST, DELETE, OPTIONS\r\nAccess-Control-Max-Age: 600\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

const JSON: &str = "application/json";
const HTML: &str = "text/html; charset=utf-8";
const TEXT: &str = "text/plain; charset=utf-8";

/// Arguments from a URL or a form: text turned into what each field's type
/// says (blank fields are left out).
fn arguments(request: &Request, tool: &str) -> Value {
    let all = tools::all();
    let fields = all.iter().find(|candidate| candidate.name == tool).map(|tool| tool.fields).unwrap_or(&[]);
    let mut out = serde_json::Map::new();
    for (key, value) in &request.query {
        if key == "token" || value.trim().is_empty() {
            continue;
        }
        let kind = fields.iter().find(|(name, ..)| name == key).map(|(_, kind, ..)| *kind).unwrap_or("string");
        let parsed = match kind {
            "integer" | "number" | "boolean" | "object" | "any" => serde_json::from_str(value.trim()).unwrap_or_else(|_| json!(value)),
            "list" => json!(value.split(',').map(str::trim).filter(|part| !part.is_empty()).collect::<Vec<_>>()),
            _ => json!(value),
        };
        out.insert(key.clone(), parsed);
    }
    Value::Object(out)
}

fn handle(stream: TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let Some(request) = read_request(&stream) else {
        return respond(&stream, "400 Bad Request", TEXT, "Bad request");
    };
    let path = request.path.trim_end_matches('/').to_string();
    let method = request.method.as_str();

    if method == "OPTIONS" {
        return respond(&stream, "204 No Content", TEXT, "");
    }
    if !request.local_host() {
        return respond(&stream, "403 Forbidden", TEXT, "This server only answers on 127.0.0.1 / localhost.");
    }
    // Says only "Paperloom's MCP server is here" (the app checks this).
    if path == "/health" {
        return respond(&stream, "200 OK", JSON, &json!({ "server": "paperloom-mcp", "version": env!("CARGO_PKG_VERSION") }).to_string());
    }

    let config = McpConfig::load();
    if request.token() != config.token {
        let wants_page = method == "GET" && !path.starts_with("/mcp") && !path.starts_with("/call") && path != "/tools";
        return if wants_page {
            respond(&stream, "401 Unauthorized", HTML, &crate::web::locked())
        } else {
            respond(&stream, "401 Unauthorized", JSON, &json!({ "ok": false, "error": "Missing or wrong token. Copy the connection details from Paperloom's MCP page." }).to_string())
        };
    }
    LAST_REQUEST.store(paperloom_common::log::now_unix(), Ordering::Relaxed);

    match (method, path.as_str()) {
        ("POST", "/mcp") => match crate::protocol::handle_text(&request.body, Gate::Serving) {
            Some(answer) => respond(&stream, "200 OK", JSON, &answer.to_string()),
            None => respond(&stream, "202 Accepted", JSON, ""),
        },
        // No server-to-client stream is offered; sessions need no clean-up.
        ("GET", "/mcp") => respond(&stream, "405 Method Not Allowed", TEXT, "POST JSON-RPC messages here."),
        ("DELETE", "/mcp") => respond(&stream, "200 OK", JSON, "{}"),
        ("GET", "") => respond(&stream, "200 OK", HTML, &crate::web::home(&config)),
        ("GET", "/guide") => respond(&stream, "200 OK", TEXT, &crate::guide::text(&config)),
        ("GET", "/tools") => respond(&stream, "200 OK", JSON, &crate::protocol::tool_list().to_string()),
        ("POST", "/shutdown") => {
            respond(&stream, "200 OK", JSON, &json!({ "ok": true }).to_string())?;
            paperloom_common::log::info("switched off");
            std::process::exit(0);
        }
        _ => {
            if let Some(tool) = path.strip_prefix("/call/") {
                // JSON in the body, or arguments in the URL.
                let args = if request.body.trim().is_empty() {
                    arguments(&request, tool)
                } else {
                    match serde_json::from_str(&request.body) {
                        Ok(args) => args,
                        Err(err) => return respond(&stream, "400 Bad Request", JSON, &json!({ "ok": false, "error": format!("The body isn't valid JSON: {err}") }).to_string()),
                    }
                };
                let answer = match tools::call(tool, &args, Gate::Serving) {
                    Ok(result) => json!({ "ok": true, "result": result }),
                    Err(error) => json!({ "ok": false, "error": error }),
                };
                respond(&stream, "200 OK", JSON, &answer.to_string())
            } else if let Some(tool) = path.strip_prefix("/run/") {
                let result = tools::call(tool, &arguments(&request, tool), Gate::Serving);
                respond(&stream, "200 OK", HTML, &crate::web::result(&config, tool, &result))
            } else {
                respond(&stream, "404 Not Found", JSON, &json!({ "ok": false, "error": "Nothing here. See /guide." }).to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_urls() {
        assert_eq!(decode("a%20b+c%2Fd"), "a b c/d");
        assert_eq!(decode("100%"), "100%");
        assert_eq!(decode("%zz"), "%zz");
        assert_eq!(parse_query("token=abc&name=Rainy+night&x="), vec![("token".into(), "abc".into()), ("name".into(), "Rainy night".into()), ("x".into(), String::new())]);
    }
}
