//! The data a skin can show, by name. Text fills `{name}` in a string; numbers
//! drive bars, rings and hands (a fraction 0–1, or an angle in degrees).

use std::collections::HashMap;
use std::fmt::Write;

use chrono::format::{Item, StrftimeItems};
use chrono::Timelike;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Text(String),
    Number(f32),
}

#[derive(Debug, Clone, Default)]
pub struct Vars(HashMap<String, Value>);

impl Vars {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn text(mut self, name: &str, value: impl Into<String>) -> Self {
        self.0.insert(name.to_string(), Value::Text(value.into()));
        self
    }

    pub fn number(mut self, name: &str, value: f32) -> Self {
        self.0.insert(name.to_string(), Value::Number(value));
        self
    }

    /// The numbers it holds (what bars, rings and hands are drawn from).
    pub fn numbers(&self) -> Vec<(String, f32)> {
        let mut numbers: Vec<(String, f32)> = self.0.iter().filter_map(|(name, value)| if let Value::Number(number) = value { Some((name.clone(), *number)) } else { None }).collect();
        numbers.sort_by(|a, b| a.0.cmp(&b.0));
        numbers
    }

    /// Equal for equal contents: nothing to redraw when it hasn't changed.
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut entries: Vec<_> = self.0.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for (name, value) in entries {
            name.hash(&mut hasher);
            match value {
                Value::Text(text) => text.hash(&mut hasher),
                Value::Number(number) => number.to_bits().hash(&mut hasher),
            }
        }
        hasher.finish()
    }

    pub fn has(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    pub fn get_number(&self, name: &str) -> f32 {
        match self.0.get(name) {
            Some(Value::Number(value)) => *value,
            Some(Value::Text(text)) => text.trim_end_matches('%').parse().unwrap_or(0.0),
            None => 0.0,
        }
    }

    /// Replaces every `{name}` in `template` with that variable's text.
    pub fn fill(&self, template: &str) -> String {
        let mut out = String::with_capacity(template.len());
        let mut rest = template;
        while let Some(start) = rest.find('{') {
            out.push_str(&rest[..start]);
            match rest[start..].find('}') {
                Some(end) => {
                    let name = &rest[start + 1..start + end];
                    if name.starts_with('%') {
                        // A date and time format, as Rainmeter and strftime write it: `{%A, %d %B}`.
                        let items: Vec<Item> = StrftimeItems::new(name).collect();
                        if !items.iter().any(|item| matches!(item, Item::Error)) {
                            let _ = write!(out, "{}", chrono::Local::now().format_with_items(items.into_iter()));
                        }
                        rest = &rest[start + end + 1..];
                        continue;
                    }
                    match self.0.get(name) {
                        Some(Value::Text(text)) => out.push_str(text),
                        Some(Value::Number(value)) if value.fract() == 0.0 => out.push_str(&format!("{}", *value as i64)),
                        Some(Value::Number(value)) => out.push_str(&format!("{value:.1}")),
                        None => {}
                    }
                    rest = &rest[start + end + 1..];
                }
                None => {
                    out.push_str(&rest[start..]);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        out
    }
}

/// Plausible values for a widget kind, for the gallery's previews.
pub fn preview(kind: &str) -> Vars {
    match kind {
        // The real time, so a preview reads like the desktop will.
        "clock" => {
            let now = chrono::Local::now();
            clock(now.hour(), now.minute(), now.second())
                .text("weekday", now.format("%A").to_string())
                .text("weekday_short", now.format("%a").to_string())
                .text("date", now.format("%A, %-d %B").to_string())
                .text("date_short", now.format("%-d %b").to_string())
        }
        "date" => {
            let now = chrono::Local::now();
            Vars::new()
                .text("weekday", now.format("%A").to_string())
                .text("weekday_short", now.format("%a").to_string())
                .text("day", now.format("%-d").to_string())
                .text("month", now.format("%B").to_string())
                .text("month_short", now.format("%b").to_string())
                .text("year", now.format("%Y").to_string())
                .text("month_year", now.format("%B %Y").to_string())
        }
        "cpu" => meter("Processor", "34%", "of all cores", Some(0.34)),
        "memory" => meter("Memory", "52%", "3.8 GB of 7.3 GB", Some(0.52)).text("used", "3.8 GB"),
        "disk" => meter("Disk", "89%", "20.0 GB free of 181.9 GB", Some(0.89)).text("free", "20.0 GB"),
        "network" => meter("Download", "1.2 MB/s", "Upload 86 KB/s", None).text("down", "1.2 MB/s").text("up", "86 KB/s"),
        "battery" => meter("Battery", "79%", "On battery", Some(0.79)),
        "uptime" => meter("Uptime", "10h 8m", "since this computer started", None),
        "weather" => meter("Weather", "24°", "Clear sky · Karachi", None).text("temp", "24").text("condition", "Clear sky").text("city", "Karachi").text("feels", "26").text("high", "29").text("low", "19").text("humidity", "48"),
        "nowplaying" => meter("Now playing", "Blue in Green", "Miles Davis", Some(0.42)).text("song", "Blue in Green").text("artist", "Miles Davis").text("position", "1:52").text("length", "5:37"),
        _ => Vars::new(),
    }
}

/// The variables every meter-style widget offers.
pub fn meter(title: &str, value: &str, detail: &str, fraction: Option<f32>) -> Vars {
    let mut vars = Vars::new().text("title", title).text("value", value).text("detail", detail);
    if let Some(percent) = value.strip_suffix('%') {
        vars = vars.text("percent", percent);
    }
    match fraction {
        Some(fraction) => {
            let fraction = fraction.clamp(0.0, 1.0);
            let lit = (fraction * 20.0).round() as usize;
            let bar = format!("{}{}", "█".repeat(lit), "░".repeat(20 - lit));
            vars.number("fraction", fraction).text("ascii_bar", bar)
        }
        None => vars,
    }
}

/// The clock's variables for a time of day (`hour` 0–23).
pub fn clock(hour: u32, minute: u32, second: u32) -> Vars {
    let hour12 = if hour.is_multiple_of(12) { 12 } else { hour % 12 };
    let minute_angle = minute as f32 * 6.0 + second as f32 * 0.1;
    Vars::new()
        .text("time", format!("{hour:02}:{minute:02}"))
        .text("time12", format!("{hour12}:{minute:02}"))
        .text("time_seconds", format!("{hour:02}:{minute:02}:{second:02}"))
        .text("hour", format!("{hour:02}"))
        .text("minute", format!("{minute:02}"))
        .text("seconds", format!("{second:02}"))
        .text("ampm", if hour < 12 { "AM" } else { "PM" })
        .number("hour_angle", (hour % 12) as f32 * 30.0 + minute as f32 * 0.5)
        .number("minute_angle", minute_angle)
        .number("second_angle", second as f32 * 6.0)
        .number("second_fraction", second as f32 / 60.0)
        .number("hour_fraction", ((hour % 12) as f32 + minute as f32 / 60.0) / 12.0)
        .number("minute_fraction", minute as f32 / 60.0)
}
