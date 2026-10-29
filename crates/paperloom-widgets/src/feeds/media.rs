//! What the music player is playing: on Linux any MPRIS player (Spotify, VLC,
//! Firefox, …) through `dbus-send`; on Windows the system's media session
//! (the same one the volume pop-up shows). Polled every few seconds while a
//! Now playing widget is placed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct Media {
    pub title: String,
    pub artist: String,
    pub playing: bool,
    /// How far into the song, 0–1, when the player says.
    pub fraction: Option<f32>,
    pub position: String,
    pub length: String,
}

static LATEST: Mutex<Option<Media>> = Mutex::new(None);
static STARTED: AtomicBool = AtomicBool::new(false);

pub fn latest() -> Option<Media> {
    LATEST.lock().ok().and_then(|latest| latest.clone())
}

pub fn start() {
    if STARTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let _ = std::thread::Builder::new().name("media".into()).spawn(|| {
        platform::prepare();
        loop {
            if super::media_wanted() {
                let found = platform::poll();
                if let Ok(mut latest) = LATEST.lock() {
                    *latest = found;
                }
            }
            std::thread::sleep(Duration::from_secs(3));
        }
    });
}

/// 215 seconds → "3:35".
fn clock(seconds: u64) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use super::{clock, Media};
    use crate::feeds::run;

    pub fn prepare() {}

    pub fn poll() -> Option<Media> {
        let names = run("dbus-send", &["--session", "--dest=org.freedesktop.DBus", "--type=method_call", "--print-reply", "/org/freedesktop/DBus", "org.freedesktop.DBus.ListNames"]).ok()?;
        let players = players(&names);
        let mut best: Option<Media> = None;
        for player in players {
            let Some(media) = read(&player) else { continue };
            // Prefer the one that is playing; else the first with a song.
            let playing = media.playing;
            if best.is_none() || playing {
                best = Some(media);
            }
            if playing {
                break;
            }
        }
        best
    }

    fn get(player: &str, property: &str) -> Option<String> {
        run(
            "dbus-send",
            &["--session", &format!("--dest={player}"), "--type=method_call", "--print-reply", "/org/mpris/MediaPlayer2", "org.freedesktop.DBus.Properties.Get", "string:org.mpris.MediaPlayer2.Player", &format!("string:{property}")],
        )
        .ok()
    }

    fn read(player: &str) -> Option<Media> {
        let metadata = parse_metadata(&get(player, "Metadata")?)?;
        let playing = get(player, "PlaybackStatus").is_some_and(|text| text.contains("\"Playing\""));
        let position = get(player, "Position").and_then(|text| number_after(&text, "int64 "));
        let fraction = match (position, metadata.length) {
            (Some(position), Some(length)) if length > 0 => Some((position as f32 / length as f32).clamp(0.0, 1.0)),
            _ => None,
        };
        Some(Media {
            title: metadata.title,
            artist: metadata.artist,
            playing,
            fraction,
            position: position.map_or(String::new(), |micros| clock((micros / 1_000_000).max(0) as u64)),
            length: metadata.length.map_or(String::new(), |micros| clock((micros / 1_000_000).max(0) as u64)),
        })
    }

    /// The MPRIS players among the bus names `ListNames` printed.
    pub(super) fn players(output: &str) -> Vec<String> {
        output.lines().filter_map(|line| line.trim().strip_prefix("string \"")?.strip_suffix('"')).filter(|name| name.starts_with("org.mpris.MediaPlayer2.")).map(str::to_string).collect()
    }

    pub(super) struct Metadata {
        pub title: String,
        pub artist: String,
        pub length: Option<i64>,
    }

    /// The first quoted string in the lines after `at`, within a few lines.
    fn quoted_after(lines: &[&str], at: usize) -> Option<String> {
        lines.iter().skip(at + 1).take(4).find_map(|line| {
            let start = line.find("string \"")? + 8;
            let end = line.rfind('"')?;
            (end >= start).then(|| line[start..end].to_string())
        })
    }

    fn number_after(text: &str, marker: &str) -> Option<i64> {
        let from = text.find(marker)? + marker.len();
        text[from..].split_whitespace().next()?.parse().ok()
    }

    pub(super) fn parse_metadata(output: &str) -> Option<Metadata> {
        let lines: Vec<&str> = output.lines().collect();
        let (mut title, mut artist, mut length) = (None, None, None);
        for (index, line) in lines.iter().enumerate() {
            let line = line.trim();
            if line == "string \"xesam:title\"" {
                title = quoted_after(&lines, index);
            } else if line == "string \"xesam:artist\"" {
                artist = quoted_after(&lines, index);
            } else if line == "string \"mpris:length\"" {
                length = lines.iter().skip(index + 1).take(3).find_map(|next| number_after(next, "int64 ").or_else(|| number_after(next, "uint64 ")));
            }
        }
        let title = title.filter(|title| !title.is_empty())?;
        Some(Metadata { title, artist: artist.unwrap_or_default(), length })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn reads_what_dbus_prints() {
            let names = "method return time=1 sender=org.freedesktop.DBus -> destination=:1.9 serial=3 reply_serial=2\n   array [\n      string \"org.freedesktop.DBus\"\n      string \"org.mpris.MediaPlayer2.spotify\"\n   ]\n";
            assert_eq!(players(names), vec!["org.mpris.MediaPlayer2.spotify".to_string()]);
            let metadata = "method return time=1 sender=:1.5 -> destination=:1.9 serial=4 reply_serial=2\n   variant       array [\n         dict entry(\n            string \"mpris:length\"\n            variant                int64 337000000\n         )\n         dict entry(\n            string \"xesam:artist\"\n            variant                array [\n                  string \"Miles Davis\"\n               ]\n         )\n         dict entry(\n            string \"xesam:title\"\n            variant                string \"Blue in Green\"\n         )\n      ]\n";
            let parsed = parse_metadata(metadata).unwrap();
            assert_eq!((parsed.title.as_str(), parsed.artist.as_str(), parsed.length), ("Blue in Green", "Miles Davis", Some(337_000_000)));
            assert!(parse_metadata("   variant array [ ]").is_none());
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::{clock, Media};
    use windows::Media::Control::{GlobalSystemMediaTransportControlsSessionManager, GlobalSystemMediaTransportControlsSessionPlaybackStatus};
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

    pub fn prepare() {
        // SAFETY: initialises WinRT for this thread, once.
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
    }

    pub fn poll() -> Option<Media> {
        let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync().ok()?.get().ok()?;
        let session = manager.GetCurrentSession().ok()?;
        let properties = session.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
        let title = properties.Title().ok()?.to_string();
        if title.is_empty() {
            return None;
        }
        let playing = session.GetPlaybackInfo().ok().and_then(|info| info.PlaybackStatus().ok()) == Some(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing);
        let timeline = session.GetTimelineProperties().ok();
        // Times are in 100-nanosecond steps.
        let seconds = |ticks: i64| (ticks / 10_000_000).max(0) as u64;
        let (position, length) = timeline.map_or((None, None), |timeline| (timeline.Position().ok().map(|time| seconds(time.Duration)), timeline.EndTime().ok().map(|time| seconds(time.Duration))));
        let fraction = match (position, length) {
            (Some(position), Some(length)) if length > 0 => Some((position as f32 / length as f32).clamp(0.0, 1.0)),
            _ => None,
        };
        Some(Media {
            title,
            artist: properties.Artist().map(|artist| artist.to_string()).unwrap_or_default(),
            playing,
            fraction,
            position: position.map_or(String::new(), clock),
            length: length.filter(|length| *length > 0).map_or(String::new(), clock),
        })
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::Media;

    pub fn prepare() {}

    pub fn poll() -> Option<Media> {
        None
    }
}
