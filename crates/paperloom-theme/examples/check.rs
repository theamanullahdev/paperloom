//! `cargo run -p paperloom-theme --example check -- <theme folder>...`
//! Checks each folder's `theme.toml` the way the app does; exits 1 if any
//! can't be used. `scripts/build-themes-index.py` runs this.

fn main() {
    let mut failed = false;
    for folder in std::env::args().skip(1) {
        let dir = std::path::Path::new(&folder);
        let report = match std::fs::read_to_string(dir.join("theme.toml")) {
            Ok(text) => paperloom_theme::check(&text, Some(dir)),
            Err(err) => {
                println!("{folder}: no theme.toml ({err})");
                failed = true;
                continue;
            }
        };
        for error in &report.errors {
            println!("{folder}: error: {error}");
        }
        for warning in &report.warnings {
            println!("{folder}: warning: {warning}");
        }
        failed |= !report.usable() || !report.warnings.is_empty();
    }
    std::process::exit(i32::from(failed));
}
