//! Digital clock with the date underneath. Changes once a minute, so it
//! wakes once a minute, on the minute.

use std::time::Duration;

use chrono::{Local, Timelike};
use tiny_skia::Pixmap;

use super::Frame;
use crate::render::{card, palette, Text};

pub fn render(scale: f32, text: &mut Text) -> Frame {
    let (w, h) = super::size_of("clock");
    let (w, h) = ((w as f32 * scale) as u32, (h as f32 * scale) as u32);
    let mut pixmap = Pixmap::new(w.max(1), h.max(1)).expect("non-empty pixmap");
    card::draw(&mut pixmap, 10.0 * scale);

    let now = Local::now();
    let time = now.format("%H:%M").to_string();
    let date = now.format("%A, %-d %B").to_string();

    let big = 56.0 * scale;
    let (time_w, _) = text.measure(&time, big, true);
    text.draw(&mut pixmap, &time, ((w as f32 - time_w) / 2.0, 8.0 * scale), big, true, palette::FOREGROUND);
    let small = 16.0 * scale;
    let (date_w, _) = text.measure(&date, small, false);
    text.draw(&mut pixmap, &date, ((w as f32 - date_w) / 2.0, 80.0 * scale), small, false, palette::MUTED);

    // Wall clock, never counted ticks: right after sleep or a clock change.
    let into_minute = Duration::new(u64::from(now.second()), now.nanosecond());
    Frame { pixmap, next_change: Duration::from_secs(60).saturating_sub(into_minute) + Duration::from_millis(20) }
}
