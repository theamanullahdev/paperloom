//! The soft translucent card every widget sits on, so it reads on any wallpaper.

use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

use super::palette;

fn color([r, g, b, a]: [u8; 4]) -> Color {
    Color::from_rgba8(r, g, b, a)
}

/// Fills the whole pixmap with a rounded card.
pub fn draw(pixmap: &mut Pixmap, radius: f32) {
    let (w, h) = (pixmap.width() as f32, pixmap.height() as f32);
    let Some(path) = rounded_rect(0.5, 0.5, w - 1.0, h - 1.0, radius) else { return };
    let mut paint = Paint { anti_alias: true, ..Paint::default() };
    paint.set_color(color(palette::CARD));
    pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    paint.set_color(color(palette::BORDER));
    pixmap.stroke_path(&path, &paint, &Stroke { width: 1.0, ..Stroke::default() }, Transform::identity(), None);
}

pub(crate) fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut b = PathBuilder::new();
    b.move_to(x + r, y);
    b.line_to(x + w - r, y);
    b.quad_to(x + w, y, x + w, y + r);
    b.line_to(x + w, y + h - r);
    b.quad_to(x + w, y + h, x + w - r, y + h);
    b.line_to(x + r, y + h);
    b.quad_to(x, y + h, x, y + h - r);
    b.line_to(x, y + r);
    b.quad_to(x, y, x + r, y);
    b.close();
    b.finish()
}
