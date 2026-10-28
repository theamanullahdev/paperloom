//! A calendar-page date: weekday, a big day number, month and year.
//! Changes at midnight.

use std::time::Duration;

use chrono::{Local, Timelike};
use tiny_skia::Pixmap;

use super::Frame;
use crate::render::{card, palette, Text};

pub fn render(scale: f32, text: &mut Text) -> Frame {
    let (w, h) = super::size_of("date");
    let (w, h) = (((w as f32 * scale) as u32).max(1), ((h as f32 * scale) as u32).max(1));
    let mut pixmap = Pixmap::new(w, h).expect("non-empty pixmap");
    card::draw(&mut pixmap, 10.0 * scale);

    let now = Local::now();
    let centered = |text: &mut Text, pixmap: &mut Pixmap, line: &str, y: f32, size: f32, bold: bool, color: [u8; 4]| {
        let (width, _) = text.measure(line, size * scale, bold);
        text.draw(pixmap, line, ((w as f32 - width) / 2.0, y * scale), size * scale, bold, color);
    };
    centered(text, &mut pixmap, &now.format("%A").to_string(), 14.0, 15.0, false, palette::ACCENT);
    centered(text, &mut pixmap, &now.format("%-d").to_string(), 34.0, 78.0, true, palette::FOREGROUND);
    centered(text, &mut pixmap, &now.format("%B %Y").to_string(), 134.0, 15.0, false, palette::MUTED);

    let seconds_left = 86_400u64.saturating_sub(u64::from(now.num_seconds_from_midnight()));
    Frame { pixmap, next_change: Duration::from_secs(seconds_left) + Duration::from_millis(50) }
}
