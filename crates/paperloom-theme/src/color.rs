//! Colours as a theme file writes them: `#RGB`, `#RRGGBB` or `#RRGGBBAA`.

/// Red, green, blue, alpha, each 0.0–1.0.
pub type Rgba = [f32; 4];

pub fn parse(text: &str) -> Option<Rgba> {
    let hex = text.trim().strip_prefix('#')?;
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let pair = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok().map(|value| f32::from(value) / 255.0);
    let single = |at: usize| u8::from_str_radix(&hex[at..at + 1], 16).ok().map(|value| f32::from(value * 17) / 255.0);
    match hex.len() {
        3 => Some([single(0)?, single(1)?, single(2)?, 1.0]),
        6 => Some([pair(0)?, pair(2)?, pair(4)?, 1.0]),
        8 => Some([pair(0)?, pair(2)?, pair(4)?, pair(6)?]),
        _ => None,
    }
}

pub fn to_hex(color: Rgba) -> String {
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    if byte(color[3]) == 255 {
        format!("#{:02X}{:02X}{:02X}", byte(color[0]), byte(color[1]), byte(color[2]))
    } else {
        format!("#{:02X}{:02X}{:02X}{:02X}", byte(color[0]), byte(color[1]), byte(color[2]), byte(color[3]))
    }
}

fn luminance(color: Rgba) -> f32 {
    let channel = |value: f32| if value <= 0.03928 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) };
    0.2126 * channel(color[0]) + 0.7152 * channel(color[1]) + 0.0722 * channel(color[2])
}

/// How well `text` reads on `background` (1 = not at all, 21 = black on white).
pub fn contrast(text: Rgba, background: Rgba) -> f32 {
    let (a, b) = (luminance(text), luminance(background));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_hex() {
        assert_eq!(parse("#fff"), Some([1.0, 1.0, 1.0, 1.0]));
        assert_eq!(parse("#FF000080").map(|c| (c[0], (c[3] * 255.0).round())), Some((1.0, 128.0)));
        assert_eq!(parse("red"), None);
        assert_eq!(parse("#12345"), None);
        assert_eq!(to_hex(parse("#9a3412").unwrap()), "#9A3412");
        assert_eq!(to_hex(parse("#FFFFFF14").unwrap()), "#FFFFFF14");
    }

    #[test]
    fn contrast_is_symmetric_and_bounded() {
        let (black, white) = ([0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0]);
        assert!((contrast(black, white) - 21.0).abs() < 0.01);
        assert!((contrast(white, black) - 21.0).abs() < 0.01);
        assert!((contrast(white, white) - 1.0).abs() < 0.01);
    }
}
