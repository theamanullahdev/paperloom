//! Primary-display aspect ratio, so the gallery's preview area can match
//! how a wallpaper will actually be cropped on screen (`panscan=1.0` in
//! the X11 engine crops-to-fill, same as CSS `background-size: cover`) —
//! a thumbnail shown at the wrong ratio hides that cropping until the
//! user sets the wallpaper for real.
//!
//! On Linux this shells out to `xrandr` rather than linking X11 bindings
//! into `paperloom-gui` just for this one query — same tradeoff already
//! made for `gdbus` in `paperloom-engine`. Windows and macOS ask the OS
//! directly (one system call each).

/// Falls back to 16:10 (this project's existing default, and a common
/// laptop panel ratio) if the size can't be read.
pub fn primary_aspect_ratio() -> (u32, u32) {
    primary_resolution().map(|(w, h)| reduce(w, h)).unwrap_or((16, 10))
}

fn reduce(width: u32, height: u32) -> (u32, u32) {
    let divisor = gcd(width, height);
    (width / divisor, height / divisor)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn primary_resolution() -> Option<(u32, u32)> {
    parse_xrandr(&run_xrandr())
}

#[cfg(windows)]
fn primary_resolution() -> Option<(u32, u32)> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    // SAFETY: plain metric queries, no pointers involved. Without DPI
    // awareness these come back scaled, but scaling keeps the ratio.
    let (width, height) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    (width > 0 && height > 0).then_some((width as u32, height as u32))
}

#[cfg(target_os = "macos")]
fn primary_resolution() -> Option<(u32, u32)> {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGMainDisplayID() -> u32;
        fn CGDisplayPixelsWide(display: u32) -> usize;
        fn CGDisplayPixelsHigh(display: u32) -> usize;
    }
    // SAFETY: CoreGraphics display queries; safe from any thread.
    let (width, height) = unsafe {
        let display = CGMainDisplayID();
        (CGDisplayPixelsWide(display), CGDisplayPixelsHigh(display))
    };
    (width > 0 && height > 0).then_some((width as u32, height as u32))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn run_xrandr() -> String {
    std::process::Command::new("xrandr")
        .arg("--current")
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default()
}

/// Looks for the primary (or else first connected) output's current mode
/// line, e.g. `eDP-1 connected primary 1680x1050+0+0 ...`.
#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
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
    Some((width, height))
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
        assert_eq!(parse_xrandr(sample), Some((1680, 1050)));
        assert_eq!(reduce(1680, 1050), (8, 5));
    }

    #[test]
    fn falls_back_when_unparsable() {
        assert_eq!(parse_xrandr(""), None);
        assert_eq!(primary_aspect_ratio(), primary_aspect_ratio());
    }
}
