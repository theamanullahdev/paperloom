//! Renders every built-in look to one PNG: `cargo run -p paperloom-skin --example sheet -- out.png`.
use paperloom_skin::{all, preview, render, Text};
use tiny_skia::{Color, Pixmap, PixmapPaint, Transform};

fn main() {
    let out = std::env::args().nth(1).expect("output path");
    let mut text = Text::new();
    let mut sheet = Pixmap::new(1400, 1000).unwrap();
    sheet.fill(Color::from_rgba8(0x3A, 0x4A, 0x6A, 255));
    let (mut x, mut y, mut row_h) = (20, 20, 0);
    for skin in all() {
        let kind = skin.kinds[0].as_str();
        let pixmap = render(skin, &preview(kind), 1.0, &mut text);
        if x + pixmap.width() as i32 + 20 > 1400 {
            x = 20;
            y += row_h + 20;
            row_h = 0;
        }
        sheet.draw_pixmap(x, y, pixmap.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        x += pixmap.width() as i32 + 20;
        row_h = row_h.max(pixmap.height() as i32);
    }
    sheet.save_png(out).unwrap();
}
