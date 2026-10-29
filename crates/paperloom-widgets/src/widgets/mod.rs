//! Turns a placed widget into a pixmap: the live data for its kind, drawn
//! through its look (`paperloom-skin`), plus how soon it needs drawing again so
//! nothing runs more often than it must.

use std::time::Duration;

use chrono::{Local, Timelike};
use paperloom_common::widgets::Placed;
use paperloom_skin::{Skin, Vars};
use tiny_skia::Pixmap;

use crate::render::Text;
use crate::sample::Sample;

pub struct Frame {
    pub pixmap: Pixmap,
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
    /// How long a meter waits before looking again.
    fn meter_interval(&self) -> Duration {
        Duration::from_secs(if self.slow { 5 } else { 2 })
    }
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
    let (vars, next_change) = data(&placed.kind, skin, context)?;
    let pixmap = paperloom_skin::render(skin, &vars, placed.scale, text);
    Some(Frame { pixmap, next_change })
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
        "cpu" => (paperloom_skin::meter("Processor", &format!("{:.0}%", sample.cpu), "of all cores", Some(sample.cpu / 100.0)), context.meter_interval()),
        "memory" => {
            let fraction = if sample.memory_total > 0 { sample.memory_used as f32 / sample.memory_total as f32 } else { 0.0 };
            let detail = format!("{} of {}", size(sample.memory_used), size(sample.memory_total));
            (paperloom_skin::meter("Memory", &format!("{:.0}%", fraction * 100.0), &detail, Some(fraction)).text("used", size(sample.memory_used)), context.meter_interval())
        }
        "disk" => {
            let fraction = if sample.disk_total > 0 { sample.disk_used as f32 / sample.disk_total as f32 } else { 0.0 };
            let detail = format!("{} free of {}", size(sample.disk_total.saturating_sub(sample.disk_used)), size(sample.disk_total));
            // Free space changes slowly.
            (paperloom_skin::meter("Disk", &format!("{:.0}%", fraction * 100.0), &detail, Some(fraction)).text("free", size(sample.disk_total.saturating_sub(sample.disk_used))), context.meter_interval() * 6)
        }
        "network" => {
            let (down, up) = (format!("{}/s", size(sample.download as u64)), format!("{}/s", size(sample.upload as u64)));
            (paperloom_skin::meter("Download", &down, &format!("Upload {up}"), None).text("down", down.clone()).text("up", up), context.meter_interval())
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
            (vars, context.meter_interval() * 6)
        }
        "uptime" => {
            let seconds = sysinfo::System::uptime();
            let (days, hours, minutes) = (seconds / 86_400, seconds / 3_600 % 24, seconds / 60 % 60);
            let value = if days > 0 { format!("{days}d {hours}h") } else if hours > 0 { format!("{hours}h {minutes}m") } else { format!("{minutes}m") };
            (paperloom_skin::meter("Uptime", &value, "since this computer started", None), context.meter_interval() * 12)
        }
        _ => return None,
    })
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
