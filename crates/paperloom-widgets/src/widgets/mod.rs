//! Turns a placed widget into a pixmap: the live data for its kind, drawn
//! through its look (`paperloom-skin`), plus how soon it needs drawing again so
//! nothing runs more often than it must.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{Local, Timelike};
use paperloom_common::widgets::Placed;
use paperloom_skin::{Skin, Vars};
use tiny_skia::Pixmap;

use crate::render::Text;
use crate::sample::Sample;

pub struct Frame {
    /// `None`: it looks exactly as it did, so nothing needs showing again.
    pub pixmap: Option<Pixmap>,
    /// How long until what is shown changes.
    pub next_change: Duration,
}

/// What a widget may look at when it is drawn.
pub struct Context<'a> {
    pub sample: &'a Sample,
    /// On battery: meters update less often.
    pub slow: bool,
}

impl Context<'_> {
    /// How long a meter waits before looking again (`times` its usual pace),
    /// up to a whole second of the wall clock, so every widget wakes together.
    fn meter(&self, times: u64) -> Duration {
        let every = if self.slow { 5 } else { 2 } * times;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        Duration::from_secs(every) - Duration::new(now.as_secs() % every, now.subsec_nanos()) + Duration::from_millis(20)
    }
}

/// How long a bar, ring or hand takes to glide to a new value.
const GLIDE: Duration = Duration::from_millis(300);
/// While gliding: about 30 frames a second.
const FRAME: Duration = Duration::from_millis(33);

/// What one widget showed last: to skip drawing the same picture twice, and
/// to glide its numbers from the old value to the new one.
#[derive(Default)]
struct Live {
    look: u64,
    drawn: Option<u64>,
    from: Vec<(String, f32)>,
    shown: Vec<(String, f32)>,
    target: Vec<(String, f32)>,
    since: Option<Instant>,
}

thread_local! {
    static LIVE: RefCell<HashMap<u32, Live>> = RefCell::new(HashMap::new());
}

/// New windows: everything is drawn afresh.
pub fn forget() {
    LIVE.with(|live| live.borrow_mut().clear());
}

/// The look a placed widget uses: the one it picked, else its kind's default.
pub fn skin_of(placed: &Placed) -> Option<&'static Skin> {
    paperloom_skin::get(&placed.skin).filter(|skin| skin.kinds.contains(&placed.kind)).or_else(|| paperloom_skin::default_for(&placed.kind))
}

/// Natural size in pixels at scale 1.0.
pub fn size_of(placed: &Placed) -> (u32, u32) {
    skin_of(placed).map_or((230, 104), |skin| (skin.size[0] as u32, skin.size[1] as u32))
}

pub fn render(placed: &Placed, text: &mut Text, context: &Context) -> Option<Frame> {
    let skin = skin_of(placed)?;
    let (mut vars, mut next_change) = data(&placed.kind, skin, context)?;
    let look = (std::ptr::from_ref(skin) as u64) ^ (u64::from(placed.scale.to_bits()) << 1);
    LIVE.with(|live| {
        let mut live = live.borrow_mut();
        let state = live.entry(placed.id).or_default();
        if state.look != look {
            *state = Live { look, ..Live::default() };
        }
        // Fractions and angles glide; on battery they jump straight there.
        let target: Vec<(String, f32)> = vars.numbers().into_iter().filter(|(name, _)| name.ends_with("fraction") || name.ends_with("angle")).collect();
        if context.slow || state.shown.len() != target.len() {
            state.shown.clone_from(&target);
            state.since = None;
        } else if target != state.target {
            state.from.clone_from(&state.shown);
            state.since = Some(Instant::now());
        }
        state.target.clone_from(&target);
        if let Some(since) = state.since {
            let t = since.elapsed().as_secs_f32() / GLIDE.as_secs_f32();
            if t >= 1.0 {
                state.shown.clone_from(&target);
                state.since = None;
            } else {
                let eased = 1.0 - (1.0 - t).powi(3);
                state.shown = state.from.iter().zip(&target).map(|((name, from), (_, to))| (name.clone(), glide(name, *from, *to, eased))).collect();
                next_change = next_change.min(FRAME);
            }
        }
        for (name, value) in &state.shown {
            vars = vars.number(name, *value);
        }
        let print = vars.fingerprint();
        if state.drawn == Some(print) {
            return Some(Frame { pixmap: None, next_change });
        }
        state.drawn = Some(print);
        Some(Frame { pixmap: Some(paperloom_skin::render(skin, &vars, placed.scale, text)), next_change })
    })
}

/// Part of the way from `from` to `to`; an angle takes the short way round.
fn glide(name: &str, from: f32, to: f32, part: f32) -> f32 {
    if name.ends_with("angle") {
        let turn = (to - from + 540.0).rem_euclid(360.0) - 180.0;
        (from + turn * part).rem_euclid(360.0)
    } else {
        from + (to - from) * part
    }
}

/// The data for one widget kind and when it next changes.
fn data(kind: &str, skin: &Skin, context: &Context) -> Option<(Vars, Duration)> {
    let sample = context.sample;
    let now = Local::now();
    Some(match kind {
        "clock" => {
            let vars = paperloom_skin::clock(now.hour(), now.minute(), now.second())
                .text("weekday", now.format("%A").to_string())
                .text("weekday_short", now.format("%a").to_string())
                .text("date", now.format("%A, %-d %B").to_string())
                .text("date_short", now.format("%-d %b").to_string());
            // Wall clock, never counted ticks: right after sleep or a clock change.
            let into = |unit: u64| Duration::new(u64::from(now.second()) % unit, now.nanosecond());
            let wait = if skin.uses_seconds { Duration::from_secs(1) - Duration::from_nanos(u64::from(now.nanosecond())) } else { Duration::from_secs(60) - into(60) };
            (vars, wait + Duration::from_millis(20))
        }
        "date" => {
            let vars = Vars::new()
                .text("weekday", now.format("%A").to_string())
                .text("weekday_short", now.format("%a").to_string())
                .text("day", now.format("%-d").to_string())
                .text("month", now.format("%B").to_string())
                .text("month_short", now.format("%b").to_string())
                .text("year", now.format("%Y").to_string())
                .text("month_year", now.format("%B %Y").to_string());
            let seconds_left = 86_400u64.saturating_sub(u64::from(now.num_seconds_from_midnight()));
            (vars, Duration::from_secs(seconds_left) + Duration::from_millis(50))
        }
        "cpu" => (paperloom_skin::meter("Processor", &format!("{:.0}%", sample.cpu), "of all cores", Some(sample.cpu / 100.0)), context.meter(1)),
        "memory" => {
            let fraction = if sample.memory_total > 0 { sample.memory_used as f32 / sample.memory_total as f32 } else { 0.0 };
            let detail = format!("{} of {}", size(sample.memory_used), size(sample.memory_total));
            (paperloom_skin::meter("Memory", &format!("{:.0}%", fraction * 100.0), &detail, Some(fraction)).text("used", size(sample.memory_used)), context.meter(1))
        }
        "disk" => {
            let fraction = if sample.disk_total > 0 { sample.disk_used as f32 / sample.disk_total as f32 } else { 0.0 };
            let detail = format!("{} free of {}", size(sample.disk_total.saturating_sub(sample.disk_used)), size(sample.disk_total));
            // Free space changes slowly.
            (paperloom_skin::meter("Disk", &format!("{:.0}%", fraction * 100.0), &detail, Some(fraction)).text("free", size(sample.disk_total.saturating_sub(sample.disk_used))), context.meter(6))
        }
        "network" => {
            let (down, up) = (format!("{}/s", size(sample.download as u64)), format!("{}/s", size(sample.upload as u64)));
            (paperloom_skin::meter("Download", &down, &format!("Upload {up}"), None).text("down", down.clone()).text("up", up), context.meter(1))
        }
        "battery" => {
            let vars = match sample.battery {
                Some(battery) => paperloom_skin::meter(
                    "Battery",
                    &format!("{}%", battery.percent),
                    if battery.discharging { "On battery" } else { "Plugged in" },
                    Some(f32::from(battery.percent) / 100.0),
                ),
                None => paperloom_skin::meter("Battery", "None", "This computer has no battery", None),
            };
            // The sampler reads the battery every 30 s.
            (vars, context.meter(6))
        }
        "weather" => match crate::feeds::weather::latest() {
            Some(weather) => {
                let unit = if paperloom_common::widgets::Widgets::load().fahrenheit { "°F" } else { "°C" };
                (
                    paperloom_skin::meter("Weather", &format!("{}°", weather.temp), &format!("{} · {}", weather.condition, weather.city), None)
                        .text("temp", weather.temp.to_string())
                        .text("unit", unit)
                        .text("condition", weather.condition.clone())
                        .text("city", weather.city.clone())
                        .text("feels", weather.feels.to_string())
                        .text("high", weather.high.to_string())
                        .text("low", weather.low.to_string())
                        .text("humidity", weather.humidity.to_string()),
                    context.meter(10),
                )
            }
            None => {
                let note = crate::feeds::weather::note();
                let detail = if note.is_empty() { "Looking up the weather…".to_string() } else { note };
                (paperloom_skin::meter("Weather", "—", &detail, None), context.meter(3))
            }
        },
        "nowplaying" => match crate::feeds::media::latest() {
            Some(media) => (
                paperloom_skin::meter(if media.playing { "Now playing" } else { "Paused" }, &shorten(&media.title, 16), &shorten(&media.artist, 30), media.fraction)
                    .text("song", media.title.clone())
                    .text("artist", media.artist.clone())
                    .text("position", media.position.clone())
                    .text("length", media.length.clone()),
                context.meter(1),
            ),
            None => (paperloom_skin::meter("Now playing", "—", "Nothing playing: start a song in your music player", None), context.meter(2)),
        },
        "uptime" => {
            let seconds = sysinfo::System::uptime();
            let (days, hours, minutes) = (seconds / 86_400, seconds / 3_600 % 24, seconds / 60 % 60);
            let value = if days > 0 { format!("{days}d {hours}h") } else if hours > 0 { format!("{hours}h {minutes}m") } else { format!("{minutes}m") };
            (paperloom_skin::meter("Uptime", &value, "since this computer started", None), context.meter(12))
        }
        _ => return None,
    })
}

/// `text` cut to `max` characters, with an ellipsis when it was longer.
fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max.saturating_sub(1)).collect::<String>().trim_end())
    }
}

/// 1536 → "1.5 KB", 5 MiB → "5 MB".
fn size(bytes: u64) -> String {
    let value = bytes as f64;
    const KB: f64 = 1024.0;
    if value >= KB * KB * KB * KB {
        format!("{:.1} TB", value / (KB * KB * KB * KB))
    } else if value >= KB * KB * KB {
        format!("{:.1} GB", value / (KB * KB * KB))
    } else if value >= KB * KB {
        format!("{:.0} MB", value / (KB * KB))
    } else if value >= KB {
        format!("{:.0} KB", value / KB)
    } else {
        format!("{bytes} B")
    }
}
