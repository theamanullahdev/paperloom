//! Data that comes from outside the computer's own counters: the weather (a
//! small request every quarter of an hour) and what the music player is
//! playing. Each runs on a thread of its own, only while a widget that shows
//! it is placed, and keeps its latest answer where the drawing code can read it
//! without ever waiting.

pub mod media;
pub mod weather;

use std::sync::atomic::{AtomicBool, Ordering};

static WEATHER_WANTED: AtomicBool = AtomicBool::new(false);
static MEDIA_WANTED: AtomicBool = AtomicBool::new(false);

/// Says which feeds the placed widgets need; starts them the first time they do.
pub fn set_needs(weather: bool, media: bool) {
    WEATHER_WANTED.store(weather, Ordering::Relaxed);
    MEDIA_WANTED.store(media, Ordering::Relaxed);
    if weather {
        weather::start();
    }
    if media {
        media::start();
    }
}

fn weather_wanted() -> bool {
    WEATHER_WANTED.load(Ordering::Relaxed)
}

fn media_wanted() -> bool {
    MEDIA_WANTED.load(Ordering::Relaxed)
}

/// Runs a command that must answer quickly and returns what it printed.
pub fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let output = paperloom_common::process::command(program).args(args).stdin(std::process::Stdio::null()).output().map_err(|err| format!("{program}: {err}"))?;
    if !output.status.success() {
        return Err(format!("{program} failed: {}", String::from_utf8_lossy(&output.stderr).lines().next().unwrap_or("no reason given")));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
