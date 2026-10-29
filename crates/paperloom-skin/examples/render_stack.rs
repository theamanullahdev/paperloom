//! Draws a stack folder's preview: `cargo run -p paperloom-skin --example render_stack -- <widgets dir> <stack folder> <out.png>`.
//! Every widget of the stack where it sits in it, on a transparent background; the looks it uses are
//! read from the catalogue folder (and the built-in ones).
use paperloom_skin::{preview, render, Text};
use tiny_skia::{Pixmap, PixmapPaint, Transform};

fn main() {
    let mut args = std::env::args().skip(1);
    let widgets = std::path::PathBuf::from(args.next().expect("widgets dir"));
    let folder = std::path::PathBuf::from(args.next().expect("stack folder"));
    let out = args.next().expect("output png");
    paperloom_skin::set_user_dir(&widgets);
    let stack: toml::Table = std::fs::read_to_string(folder.join("stack.toml")).expect("stack.toml").parse().expect("valid stack.toml");
    let size = stack["size"].as_array().expect("size");
    let (w, h) = (size[0].as_float().or(size[0].as_integer().map(|n| n as f64)).unwrap() as u32, size[1].as_float().or(size[1].as_integer().map(|n| n as f64)).unwrap() as u32);
    let mut canvas = Pixmap::new(w, h).expect("canvas");
    let mut text = Text::new();
    let number = |value: &toml::Value| value.as_float().or(value.as_integer().map(|n| n as f64)).unwrap() as i32;
    for widget in stack["widget"].as_array().expect("widgets") {
        let kind = widget["kind"].as_str().expect("kind");
        let skin = widget.get("skin").and_then(|skin| skin.as_str()).and_then(paperloom_skin::get).or_else(|| paperloom_skin::default_for(kind)).expect("look");
        let picture = render(skin, &preview(kind), 1.0, &mut text);
        canvas.draw_pixmap(number(&widget["dx"]), number(&widget["dy"]), picture.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    }
    canvas.save_png(out).unwrap();
}
