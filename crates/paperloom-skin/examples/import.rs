//! Imports a Rainmeter skin and draws each look it made:
//! `cargo run -p paperloom-skin --example import -- <skin> <looks dir>`.

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(from), Some(into)) = (args.next(), args.next()) else {
        eprintln!("usage: import <rmskin|folder|ini> <looks dir>");
        std::process::exit(2);
    };
    let into = std::path::PathBuf::from(into);
    match paperloom_skin::rainmeter::import(std::path::Path::new(&from), &into) {
        Ok(report) => {
            println!("added: {:?}", report.looks);
            for note in &report.left_out {
                println!("left out: {note}");
            }
        }
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    }
    paperloom_skin::set_user_dir(&into);
    let mut text = paperloom_skin::Text::new();
    for skin in paperloom_skin::all().into_iter().filter(|skin| skin.dir.is_some()) {
        let kind = skin.kinds.first().map_or("clock", String::as_str);
        text.ensure_fonts(skin);
        let pixmap = paperloom_skin::render(skin, &paperloom_skin::preview(kind), 1.0, &mut text);
        let file = into.join(format!("{}.png", skin.id.replace('/', "_")));
        pixmap.save_png(&file).ok();
        println!("drew {}", file.display());
    }
}
