//! The MCP server's settings, and switching it on and off.
//!
//! `paperloom-mcp` lets an AI agent on this computer do what the window
//! does. It is a separate program that only runs after the user switches it
//! on (the app's MCP page), sleeps in `accept()` between requests, and is
//! never started at login. "On" simply means "that program is running and
//! answers on its port"; nothing else in Paperloom depends on it.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::paths;

pub const DEFAULT_PORT: u16 = 47821;

/// `mcp.toml`: read by the server on every request, so a switch flipped in
/// the app applies to the next call without a restart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct McpConfig {
    /// The local port the server listens on (127.0.0.1 only).
    pub port: u16,
    /// What an agent must present. Kept between runs so an agent's saved
    /// connection keeps working; "New token" in the app replaces it.
    pub token: String,
    /// Off = the agent can look but not touch.
    pub allow_changes: bool,
    /// Removing wallpapers and deleting collections.
    pub allow_remove: bool,
    /// Changing Paperloom's settings.
    pub allow_settings: bool,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self { port: DEFAULT_PORT, token: String::new(), allow_changes: true, allow_remove: false, allow_settings: false }
    }
}

impl McpConfig {
    /// The saved file, with a token made (and saved) the first time.
    pub fn load() -> Self {
        let mut config: Self = std::fs::read_to_string(paths::mcp_file()).ok().and_then(|text| toml::from_str(&text).ok()).unwrap_or_default();
        if config.port == 0 {
            config.port = DEFAULT_PORT;
        }
        if config.token.len() < 16 {
            config.token = new_token();
            let _ = config.save();
        }
        config
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = paths::mcp_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, toml::to_string_pretty(self).expect("McpConfig always serializes"))?;
        // It holds the token: readable by the user only.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    /// Where agents connect.
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

/// 32 hex characters nobody can guess. `RandomState` is seeded by the
/// operating system's random source; the clock and pid are mixed in too.
pub fn new_token() -> String {
    use sha2::{Digest, Sha256};
    use std::hash::{BuildHasher, Hasher};
    let mut hash = Sha256::new();
    for _ in 0..4 {
        hash.update(std::collections::hash_map::RandomState::new().build_hasher().finish().to_le_bytes());
    }
    hash.update(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0).to_le_bytes());
    hash.update(std::process::id().to_le_bytes());
    hash.finalize().iter().take(16).map(|byte| format!("{byte:02x}")).collect()
}

/// One small request to the local server; the reply's body, if it answered.
fn request(port: u16, method: &str, path: &str, token: &str) -> Option<String> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(400)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    stream.set_write_timeout(Some(Duration::from_secs(2))).ok()?;
    write!(stream, "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").ok()?;
    let mut reply = String::new();
    stream.take(64 * 1024).read_to_string(&mut reply).ok()?;
    reply.split_once("\r\n\r\n").map(|(_, body)| body.to_string())
}

/// Is Paperloom's MCP server answering on `port`? (Something else that
/// happens to use the port doesn't count.)
pub fn is_running_on(port: u16) -> bool {
    request(port, "GET", "/health", "").is_some_and(|body| body.contains("\"paperloom-mcp\""))
}

/// Is MCP switched on right now? With it off this is one refused local
/// connection, a few microseconds.
pub fn is_running() -> bool {
    if !paths::mcp_file().is_file() {
        return false;
    }
    is_running_on(McpConfig::load().port)
}

/// The instructions an agent needs (address, token, every way to connect,
/// the tool list), from the running server. `None` while MCP is off.
pub fn guide() -> Option<String> {
    let config = McpConfig::load();
    request(config.port, "GET", "/guide", &config.token).filter(|text| text.contains("Paperloom"))
}

/// Switches MCP on: starts `paperloom-mcp serve` (next to this program) and
/// waits until it answers. Already on is fine.
pub fn start() -> std::io::Result<()> {
    if is_running() {
        return Ok(());
    }
    let program = crate::process::sibling_binary("paperloom-mcp").ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "paperloom-mcp isn't installed next to Paperloom"))?;
    let mut command = std::process::Command::new(program);
    crate::process::hide_console(&mut command);
    command.arg("serve").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let mut child = command.spawn()?;
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(80));
        // The port can move if the saved one is taken, so read it afresh.
        if is_running() {
            return Ok(());
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(std::io::Error::other(format!("the MCP server stopped right away ({status})")));
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "the MCP server didn't answer"))
}

/// Switches MCP off: asks the server to end. Not running is fine.
pub fn stop() {
    if !paths::mcp_file().is_file() {
        return;
    }
    let config = McpConfig::load();
    if request(config.port, "POST", "/shutdown", &config.token).is_some() {
        let started = std::time::Instant::now();
        while started.elapsed() < Duration::from_secs(2) && is_running_on(config.port) {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_long_and_differ() {
        let (a, b) = (new_token(), new_token());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }

    #[test]
    fn nothing_listening_is_not_running() {
        // Port 9 (discard) on localhost: closed on any normal machine.
        assert!(!is_running_on(9));
    }
}
