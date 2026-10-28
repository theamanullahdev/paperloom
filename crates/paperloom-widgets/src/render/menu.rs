//! The small menu a widget shows when it is right-clicked: drawn by the widget
//! itself (inside its own window), so it looks the same on every platform and
//! needs no toolkit. It closes when a choice is made or a click lands elsewhere.
//! (Windows shows its own native menu from the same items.)

#![cfg_attr(windows, allow(dead_code))]

use tiny_skia::{Color, FillRule, Paint, Pixmap, Transform};

use super::{card, palette, Text};

pub const WIDTH: u32 = 220;
const ITEM: f32 = 32.0;
const PAD: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Open this widget's settings in the app.
    Settings,
    /// Lock or unlock dragging.
    ToggleLock,
    Remove,
    /// Switch every widget off.
    HideAll,
}

pub fn items(drag: bool) -> Vec<(Choice, &'static str)> {
    vec![
        (Choice::Settings, "Settings…"),
        (Choice::ToggleLock, if drag { "Lock position" } else { "Unlock position" }),
        (Choice::Remove, "Remove from desktop"),
        (Choice::HideAll, "Hide all widgets"),
    ]
}

pub fn size(count: usize) -> (u32, u32) {
    (WIDTH, (PAD * 2.0 + ITEM * count as f32) as u32)
}

/// Which item a pointer position (in the menu's own pixels) is over.
pub fn hit(count: usize, x: f32, y: f32) -> Option<usize> {
    if x < 0.0 || x > WIDTH as f32 || y < PAD {
        return None;
    }
    let index = ((y - PAD) / ITEM) as usize;
    (index < count).then_some(index)
}

pub fn draw(items: &[(Choice, &str)], hover: Option<usize>, text: &mut Text) -> Pixmap {
    let (w, h) = size(items.len());
    let mut pixmap = Pixmap::new(w, h).expect("non-empty pixmap");
    card::draw(&mut pixmap, 8.0);
    for (index, (choice, label)) in items.iter().enumerate() {
        let top = PAD + ITEM * index as f32;
        if hover == Some(index) {
            let mut paint = Paint { anti_alias: true, ..Paint::default() };
            paint.set_color(Color::from_rgba8(0xFB, 0x92, 0x3C, 0x33));
            if let Some(path) = card::rounded_rect(PAD, top + 2.0, w as f32 - PAD * 2.0, ITEM - 4.0, 5.0) {
                pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
            }
        }
        let color = if *choice == Choice::Remove { [0xF8, 0x71, 0x71, 0xFF] } else { palette::FOREGROUND };
        text.draw(&mut pixmap, label, (PAD + 12.0, top + 7.0), 15.0, false, color);
    }
    pixmap
}
