//! Draws a look folder's preview: `cargo run -p paperloom-skin --example render -- <folder> <out.png>`.
//! Used to make the catalogue's `preview.png` files, so previews are always what the renderer draws.
use paperloom_skin::{preview, render, Skin, Text};
use tiny_skia::{Color, GradientStop, LinearGradient, Paint, Pixmap, PixmapPaint, Point, Rect, Shader, SpreadMode, Transform};

fn main() {
    let mut args = std::env::args().skip(1);
    let folder = std::path::PathBuf::from(args.next().expect("look folder"));
    let out = args.next().expect("output png");
    let mut skin = Skin::parse(&std::fs::read_to_string(folder.join("skin.toml")).expect("skin.toml")).expect("valid skin");
    skin.dir = Some(folder);
    let kind = skin.kinds.first().expect("a kind").clone();
    let widget = render(&skin, &preview(&kind), 1.0, &mut Text::new());
    let pad = 28u32;
    let (w, h) = (widget.width() + pad * 2, widget.height() + pad * 2);
    let mut backdrop = Pixmap::new(w, h).unwrap();
    let shader = LinearGradient::new(
        Point::from_xy(0.0, 0.0),
        Point::from_xy(w as f32, h as f32),
        vec![GradientStop::new(0.0, Color::from_rgba8(0x16, 0x20, 0x3d, 255)), GradientStop::new(1.0, Color::from_rgba8(0x4a, 0x24, 0x5e, 255))],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap_or(Shader::SolidColor(Color::BLACK));
    let paint = Paint { shader, ..Paint::default() };
    backdrop.fill_rect(Rect::from_xywh(0.0, 0.0, w as f32, h as f32).unwrap(), &paint, Transform::identity(), None);
    backdrop.draw_pixmap(pad as i32, pad as i32, widget.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    backdrop.save_png(out).unwrap();
}
