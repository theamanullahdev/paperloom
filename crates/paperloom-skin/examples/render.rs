//! Draws a look folder's preview: `cargo run -p paperloom-skin --example render -- <folder> <out.png>`.
//! Used to make the catalogue's `preview.png` files, so previews are always what the renderer draws.
//! The widget alone, on a transparent background: the app draws it on the user's own wallpaper.
use paperloom_skin::{preview, render, Skin, Text};

fn main() {
    let mut args = std::env::args().skip(1);
    let folder = std::path::PathBuf::from(args.next().expect("look folder"));
    let out = args.next().expect("output png");
    let mut skin = Skin::parse(&std::fs::read_to_string(folder.join("skin.toml")).expect("skin.toml")).expect("valid skin");
    skin.dir = Some(folder);
    let kind = skin.kinds.first().expect("a kind").clone();
    render(&skin, &preview(&kind), 1.0, &mut Text::new()).save_png(out).unwrap();
}
