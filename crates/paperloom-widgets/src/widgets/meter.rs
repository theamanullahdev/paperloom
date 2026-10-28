//! The shared layout of the small meter widgets: a title, a big value, a line
//! of detail and an optional bar.

use tiny_skia::Pixmap;

use crate::render::{card, palette, Text};

pub fn draw(kind: &str, scale: f32, text: &mut Text, title: &str, value: &str, detail: &str, fraction: Option<f32>) -> Pixmap {
    let (w, h) = super::size_of(kind);
    let (w, h) = (((w as f32 * scale) as u32).max(1), ((h as f32 * scale) as u32).max(1));
    let mut pixmap = Pixmap::new(w, h).expect("non-empty pixmap");
    card::draw(&mut pixmap, 10.0 * scale);
    text.draw(&mut pixmap, title, (14.0 * scale, 10.0 * scale), 12.0 * scale, false, palette::MUTED);
    text.draw(&mut pixmap, value, (14.0 * scale, 26.0 * scale), 32.0 * scale, true, palette::FOREGROUND);
    text.draw(&mut pixmap, detail, (14.0 * scale, 68.0 * scale), 12.0 * scale, false, palette::MUTED);
    if let Some(fraction) = fraction {
        card::bar(&mut pixmap, 14.0 * scale, h as f32 - 18.0 * scale, w as f32 - 28.0 * scale, 6.0 * scale, fraction);
    }
    pixmap
}

/// 1536 → "1.5 KB", 5 MiB → "5.0 MB".
pub fn size(bytes: u64) -> String {
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

pub fn speed(bytes_per_second: f64) -> String {
    format!("{}/s", size(bytes_per_second as u64))
}
