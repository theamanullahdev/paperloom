//! Anonymous usage counts: "someone installed this look", "the window was
//! opened today", "a feature was used". Nothing here can identify a person:
//! a random id made on this computer (never derived from the user, the
//! machine or the network), the app version, the system and CPU kind, and one
//! of a fixed list of names. No wallpaper titles, paths, links or text.
//!
//! Each count is one short request from a throw-away thread; the app never
//! waits for it and ignores every failure. Settings → Privacy switches all of
//! it off (`share_counts`).

use std::path::PathBuf;
use std::time::Duration;

use crate::http;

/// The counter service (the `paperloom-server` project).
const URL: &str = "https://paperloom-counts.vercel.app/api/count";

/// The windows' version, e.g. `0.45.0`.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Someone installed `id` (a `look`, `stack` or `theme` from the online library).
pub fn install(kind: &str, id: &str) {
    send(serde_json::json!({ "event": "install", "kind": kind, "id": id }));
}

/// The window was opened; at most one count a day.
pub fn opened() {
    let today = days_since_epoch();
    let file = last_open_file();
    if std::fs::read_to_string(&file).ok().and_then(|text| text.trim().parse::<u64>().ok()) == Some(today) {
        return;
    }
    if !enabled() {
        return;
    }
    let _ = std::fs::write(&file, today.to_string());
    let os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    send(serde_json::json!({ "event": "open", "version": VERSION, "os": os, "arch": std::env::consts::ARCH }));
}

/// A feature was used. `name` is one of the fixed names the service knows.
pub fn feature(name: &str) {
    send(serde_json::json!({ "event": "feature", "name": name }));
}

/// The feature name for a download from `source` (a source id such as `motionbgs.com`).
pub fn source_feature(source: &str) -> &'static str {
    let source = source.to_lowercase();
    for (word, name) in [
        ("motionbgs", "source_motionbgs"),
        ("pexels", "source_pexels"),
        ("pixabay", "source_pixabay"),
        ("mixkit", "source_mixkit"),
        ("wallpaperwaves", "source_wallpaperwaves"),
    ] {
        if source.contains(word) {
            return name;
        }
    }
    "source_custom"
}

fn enabled() -> bool {
    !cfg!(test) && std::env::var_os("PAPERLOOM_NO_COUNTS").is_none() && paperloom_common::load().share_counts
}

fn send(mut body: serde_json::Value) {
    if !enabled() {
        return;
    }
    let Some(client) = client_id() else { return };
    body["client"] = serde_json::Value::String(client);
    let _ = std::thread::Builder::new().name("paperloom-counts".into()).spawn(move || {
        let request = http::agent().post(URL).config().timeout_global(Some(Duration::from_secs(5))).build();
        let _ = request.header("Content-Type", "application/json").send(body.to_string());
    });
}

/// The random id of this installation, made the first time it is needed.
fn client_id() -> Option<String> {
    let file = paperloom_common::counts_id_file();
    if let Ok(text) = std::fs::read_to_string(&file) {
        let text = text.trim();
        if text.len() == 32 && text.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Some(text.to_lowercase());
        }
    }
    let id = paperloom_common::mcp::new_token();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&file, &id).ok()?;
    Some(id)
}

fn last_open_file() -> PathBuf {
    paperloom_common::counts_id_file().with_file_name("counts-last-open")
}

fn days_since_epoch() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() / 86_400)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_map_to_fixed_names() {
        assert_eq!(source_feature("motionbgs.com"), "source_motionbgs");
        assert_eq!(source_feature("Mixkit"), "source_mixkit");
        assert_eq!(source_feature("https://my.site/{q}"), "source_custom");
    }

    #[test]
    fn nothing_is_sent_while_testing() {
        assert!(!enabled());
    }
}
