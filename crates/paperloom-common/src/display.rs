//! Primary-display aspect ratio, so the gallery's preview area can match
//! how a wallpaper will actually be cropped on screen (`panscan=1.0` in
//! the X11 engine crops-to-fill, same as CSS `background-size: cover`) —
//! a thumbnail shown at the wrong ratio hides that cropping until the
//! user sets the wallpaper for real.
//!
//! Shells out to `xrandr` rather than linking X11 bindings into
//! `paperloom-gui` just for this one query — same tradeoff already made
//! for `gdbus`/`powerprofilesctl` in `paperloom-engine`.

use std::process::Command;

/// Falls back to 16:10 (this project's existing default, and a common
/// laptop panel ratio) if `xrandr` is missing or its output doesn't parse.
pub fn primary_aspect_ratio() -> (u32, u32) {
    parse_xrandr(&run_xrandr()).unwrap_or((16, 10))
}

fn run_xrandr() -> String {
    Command::new("xrandr")
        .arg("--current")
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default()
}

/// Looks for the primary (or else first connected) output's current mode
/// line, e.g. `eDP-1 connected primary 1680x1050+0+0 ...`.
fn parse_xrandr(output: &str) -> Option<(u32, u32)> {
    let line = output
        .lines()
        .find(|line| line.contains("connected primary"))
        .or_else(|| output.lines().find(|line| line.contains(" connected ")))?;

    let resolution = line.split_whitespace().find(|token| token.contains('x') && token.chars().next().is_some_and(|c| c.is_ascii_digit()))?;
    let resolution = resolution.split('+').next()?;
    let (width, height) = resolution.split_once('x')?;
    let (width, height) = (width.parse::<u32>().ok()?, height.parse::<u32>().ok()?);
    if width == 0 || height == 0 {
        return None;
    }

    let divisor = gcd(width, height);
    Some((width / divisor, height / divisor))
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_primary_output_resolution() {
        let sample = "eDP-1 connected primary 1680x1050+0+0 (normal left inverted right x axis y axis) 301mm x 188mm\n   1920x1200     60.00 +  60.00    48.00\n";
        assert_eq!(parse_xrandr(sample), Some((8, 5)));
    }

    #[test]
    fn falls_back_when_unparsable() {
        assert_eq!(parse_xrandr(""), None);
        assert_eq!(primary_aspect_ratio(), primary_aspect_ratio());
    }
}
