//! A whole agent session against the real program, in a scratch home: the
//! switch, the token, MCP over HTTP and over stdin/stdout, the JSON API,
//! the web page, an import, the permission switches, and switching off.
//! Linux only: that is where the XDG variables move every Paperloom folder.
#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const PROGRAM: &str = env!("CARGO_BIN_EXE_paperloom-mcp");

struct Home(PathBuf);

impl Home {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("paperloom-mcp-test-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        for sub in ["config", "data", "cache", "run"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        Home(dir)
    }

    /// The program, seeing only this scratch home (and no real engine socket).
    fn program(&self) -> Command {
        let mut command = Command::new(PROGRAM);
        command
            .env("HOME", &self.0)
            .env("XDG_CONFIG_HOME", self.0.join("config"))
            .env("XDG_DATA_HOME", self.0.join("data"))
            .env("XDG_CACHE_HOME", self.0.join("cache"))
            .env("XDG_RUNTIME_DIR", self.0.join("run"))
            .env_remove("FLATPAK_ID");
        command
    }

    fn config_file(&self) -> PathBuf {
        self.0.join("config/paperloom/mcp.toml")
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// One HTTP request; returns (status code, body).
fn http(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let mut request = format!("{method} {path} HTTP/1.1\r\nContent-Length: {}\r\n", body.len());
    if !headers.iter().any(|(name, _)| name.eq_ignore_ascii_case("host")) {
        request.push_str(&format!("Host: 127.0.0.1:{port}\r\n"));
    }
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(body);
    stream.write_all(request.as_bytes()).unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    let status = reply.split_whitespace().nth(1).and_then(|code| code.parse().ok()).unwrap_or(0);
    (status, reply.split_once("\r\n\r\n").map(|(_, body)| body.to_string()).unwrap_or_default())
}

fn call(port: u16, token: &str, tool: &str, args: Value) -> Value {
    let (status, body) = http(port, "POST", &format!("/call/{tool}"), &[("Authorization", &format!("Bearer {token}")), ("Content-Type", "application/json")], &args.to_string());
    assert_eq!(status, 200, "{tool}: {body}");
    serde_json::from_str(&body).unwrap_or_else(|err| panic!("{tool}: {err}: {body}"))
}

fn token_in(file: &Path) -> String {
    let text = std::fs::read_to_string(file).unwrap();
    text.lines().find_map(|line| line.strip_prefix("token = ")).unwrap().trim_matches('"').to_string()
}

#[test]
fn an_agent_session_end_to_end() {
    let home = Home::new();

    // --- Off: nothing listens, and the other ways in say so.
    let off = home.program().args(["call", "status"]).output().unwrap();
    assert!(!off.status.success());
    assert!(String::from_utf8_lossy(&off.stderr).contains("switched off"), "{off:?}");

    // --- On.
    let port = TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
    let server = Server(home.program().args(["serve", "--port", &port.to_string()]).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap());
    let started = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(started.elapsed() < Duration::from_secs(10), "the server never listened");
        std::thread::sleep(Duration::from_millis(30));
    }
    let token = token_in(&home.config_file());
    assert_eq!(token.len(), 32);
    let bearer = format!("Bearer {token}");

    // --- The door: health is open, everything else needs the token and a local Host.
    let (status, body) = http(port, "GET", "/health", &[], "");
    assert_eq!(status, 200);
    assert!(body.contains("paperloom-mcp"));
    assert_eq!(http(port, "GET", "/tools", &[], "").0, 401);
    assert_eq!(http(port, "POST", "/call/status", &[("Authorization", "Bearer wrong")], "").0, 401);
    assert_eq!(http(port, "GET", "/tools", &[("Host", "evil.example"), ("Authorization", &bearer)], "").0, 403);
    assert_eq!(http(port, "OPTIONS", "/mcp", &[("Origin", "https://example.com")], "").0, 204);

    // --- MCP over HTTP.
    let rpc = |message: Value| -> (u16, String) { http(port, "POST", "/mcp", &[("Authorization", &bearer), ("Content-Type", "application/json"), ("Accept", "application/json, text/event-stream")], &message.to_string()) };
    let (status, body) = rpc(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } } }));
    assert_eq!(status, 200);
    let answer: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(answer["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(rpc(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).0, 202);
    let (_, body) = rpc(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "status", "arguments": {} } }));
    let answer: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(answer["result"]["isError"], false, "{body}");
    assert_eq!(answer["result"]["structuredContent"]["engine_running"], false);
    // The token in the URL works too (clients that can't set headers).
    let (status, _) = http(port, "POST", &format!("/mcp?token={token}"), &[], &json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }).to_string());
    assert_eq!(status, 200);

    // --- Import a file an agent "downloaded itself".
    let video = home.0.join("Rainy night.mp4");
    std::fs::write(&video, [b"\0\0\0\x18ftypmp42".as_slice(), &[7u8; 4096]].concat()).unwrap();
    let imported = call(port, &token, "import_file", json!({ "path": video, "name": "Rainy night", "collection": "Moody" }));
    assert_eq!(imported["ok"], true, "{imported}");
    assert_eq!(imported["result"]["name"], "Rainy night");
    assert_eq!(imported["result"]["already_in_library"], false);
    assert_eq!(imported["result"]["collections"], json!(["Moody"]));
    let id = imported["result"]["id"].as_u64().unwrap();
    // The same file again is recognised, not doubled.
    assert_eq!(call(port, &token, "import_file", json!({ "path": video }))["result"]["already_in_library"], true);
    let listed = call(port, &token, "list_wallpapers", json!({ "query": "rainy" }));
    assert_eq!(listed["result"]["total"], 1);
    // Not a video or picture: refused with the reason.
    let text_file = home.0.join("notes.txt");
    std::fs::write(&text_file, "hello").unwrap();
    assert!(call(port, &token, "import_file", json!({ "path": text_file }))["error"].as_str().unwrap().contains(".txt"));

    // --- Changes: favorite, tags, hidden (left out of lists unless asked for).
    assert_eq!(call(port, &token, "set_favorite", json!({ "wallpaper": id, "favorite": true }))["result"]["favorite"], true);
    assert_eq!(call(port, &token, "set_tags", json!({ "wallpaper": "rainy", "tags": ["Rain", "night"] }))["result"]["tags"], json!(["rain", "night"]));
    call(port, &token, "set_hidden", json!({ "wallpaper": id, "hidden": true }));
    assert_eq!(call(port, &token, "list_wallpapers", json!({}))["result"]["total"], 0);
    assert_eq!(call(port, &token, "list_wallpapers", json!({ "include_hidden": true }))["result"]["total"], 1);
    assert_eq!(call(port, &token, "list_collections", json!({}))["result"]["collections"][0]["wallpapers"], 1);

    // --- The web page, as a browser agent uses it: a form per tool, results as pages.
    let (status, page) = http(port, "GET", &format!("/?token={token}"), &[], "");
    assert_eq!(status, 200);
    assert!(page.contains("action=\"/run/import_file\""));
    let (_, page) = http(port, "GET", &format!("/run/set_favorite?token={token}&wallpaper={id}&favorite=false"), &[], "");
    assert!(page.contains("Done") && page.contains("&quot;favorite&quot;: false"), "{page}");
    assert_eq!(http(port, "GET", "/", &[], "").0, 401);

    // --- stdin/stdout, while switched on.
    let mut agent = home.program().stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut input = agent.stdin.take().unwrap();
    let mut output = BufReader::new(agent.stdout.take().unwrap());
    let mut ask = |message: Value| -> Value {
        writeln!(input, "{message}").unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    };
    assert_eq!(ask(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2024-11-05" } }))["result"]["protocolVersion"], "2024-11-05");
    let seen = ask(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "get_wallpaper", "arguments": { "wallpaper": id } } }));
    assert_eq!(seen["result"]["structuredContent"]["name"], "Rainy night", "{seen}");
    drop(input);
    agent.wait().unwrap();

    // --- Permission switches, read afresh on every call.
    let refused = call(port, &token, "remove_wallpaper", json!({ "wallpaper": id, "confirm": true }));
    assert!(refused["error"].as_str().unwrap().contains("Allow removing"), "{refused}");
    assert!(call(port, &token, "set_setting", json!({ "key": "muted", "value": false }))["error"].as_str().unwrap().contains("Allow changing settings"));
    let config = std::fs::read_to_string(home.config_file()).unwrap().replace("allow_remove = false", "allow_remove = true").replace("allow_settings = false", "allow_settings = true");
    std::fs::write(home.config_file(), config).unwrap();
    assert!(call(port, &token, "remove_wallpaper", json!({ "wallpaper": id }))["error"].as_str().unwrap().contains("confirm: true"));
    assert_eq!(call(port, &token, "remove_wallpaper", json!({ "wallpaper": id, "confirm": true }))["ok"], true);
    assert_eq!(call(port, &token, "list_wallpapers", json!({ "include_hidden": true }))["result"]["total"], 0);
    assert_eq!(call(port, &token, "set_setting", json!({ "key": "pause_on_low_battery", "value": false }))["result"]["value"], false);
    assert_eq!(call(port, &token, "get_settings", json!({}))["result"]["pause_on_low_battery"], false);
    assert!(call(port, &token, "set_setting", json!({ "key": "hidden_lock", "value": "x" }))["error"].as_str().unwrap().contains("own window"));
    let read_only = std::fs::read_to_string(home.config_file()).unwrap().replace("allow_changes = true", "allow_changes = false");
    std::fs::write(home.config_file(), read_only).unwrap();
    assert!(call(port, &token, "create_collection", json!({ "name": "x" }))["error"].as_str().unwrap().contains("Allow changes"));
    assert_eq!(call(port, &token, "status", json!({}))["ok"], true);

    // --- Widgets and rotation are plain data too (nothing is started: widgets stay off).
    let on = std::fs::read_to_string(home.config_file()).unwrap().replace("allow_changes = false", "allow_changes = true");
    std::fs::write(home.config_file(), on).unwrap();
    let added = call(port, &token, "add_widget", json!({ "kind": "cpu", "x": 0.5, "y": 0.25 }));
    assert_eq!(added["result"]["widget"]["kind"], "cpu", "{added}");
    assert_eq!(added["result"]["widget"]["x"], 0.5);
    assert!(call(port, &token, "set_rotation", json!({ "changes": { "interval_minute": 5 } }))["error"].as_str().unwrap().contains("interval_minute"));
    assert_eq!(call(port, &token, "set_rotation", json!({ "changes": { "interval_minutes": 5, "shuffle": true } }))["result"]["interval_minutes"], 5);

    // --- Off again: the server ends and the port is free.
    assert_eq!(http(port, "POST", "/shutdown", &[("Authorization", &bearer)], "").0, 200);
    let mut server = server;
    let ended = Instant::now();
    while server.0.try_wait().unwrap().is_none() {
        assert!(ended.elapsed() < Duration::from_secs(5), "the server didn't end");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
}
