//! The data a skin can show, by name. Text fills `{name}` in a string; numbers
//! drive bars, rings and hands (a fraction 0–1, or an angle in degrees).

use std::collections::HashMap;

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
        "clock" => clock(10, 8, 24).text("weekday", "Thursday").text("weekday_short", "Thu").text("date", "Thursday, 29 October").text("date_short", "29 Oct"),
        "date" => Vars::new()
            .text("weekday", "Thursday")
            .text("weekday_short", "Thu")
            .text("day", "29")
            .text("month", "October")
            .text("month_short", "Oct")
            .text("year", "2026")
            .text("month_year", "October 2026"),
        "cpu" => meter("Processor", "34%", "of all cores", Some(0.34)),
        "memory" => meter("Memory", "52%", "3.8 GB of 7.3 GB", Some(0.52)),
        "disk" => meter("Disk", "89%", "20.0 GB free of 181.9 GB", Some(0.89)),
        "network" => meter("Download", "1.2 MB/s", "Upload 86 KB/s", None),
        "battery" => meter("Battery", "79%", "On battery", Some(0.79)),
        "uptime" => meter("Uptime", "10h 8m", "since this computer started", None),
        _ => Vars::new(),
    }
}

/// The variables every meter-style widget offers.
pub fn meter(title: &str, value: &str, detail: &str, fraction: Option<f32>) -> Vars {
    let vars = Vars::new().text("title", title).text("value", value).text("detail", detail);
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
