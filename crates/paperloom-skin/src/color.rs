//! Colours in a skin: `#rgb`, `#rrggbb`, `#rrggbbaa`, or a theme token
//! (`$card`, `$border`, `$fg`, `$muted`, `$accent`, `$track`), so a skin follows
//! Paperloom's palette unless it chooses its own.

use tiny_skia::{Color, GradientStop, LinearGradient, Point, Shader, SpreadMode, Transform};

pub fn parse(spec: &str) -> Color {
    if let Some(token) = spec.strip_prefix('$') {
        return match token {
            "card" => Color::from_rgba8(0x26, 0x22, 0x20, 0xD0),
            "border" => Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x14),
            "fg" => Color::from_rgba8(0xF5, 0xEF, 0xE9, 0xFF),
            "muted" => Color::from_rgba8(0xA8, 0x99, 0x8E, 0xFF),
            "accent" => Color::from_rgba8(0xFB, 0x92, 0x3C, 0xFF),
            "track" => Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x1A),
            _ => Color::TRANSPARENT,
        };
    }
    let hex = spec.strip_prefix('#').unwrap_or(spec);
    let nibble = |index: usize| hex.get(index..=index).and_then(|digit| u8::from_str_radix(digit, 16).ok());
    let byte = |index: usize| hex.get(index..index + 2).and_then(|pair| u8::from_str_radix(pair, 16).ok());
    match hex.len() {
        3 => match (nibble(0), nibble(1), nibble(2)) {
            (Some(r), Some(g), Some(b)) => Color::from_rgba8(r * 17, g * 17, b * 17, 255),
            _ => Color::TRANSPARENT,
        },
        6 | 8 => match (byte(0), byte(2), byte(4)) {
            (Some(r), Some(g), Some(b)) => Color::from_rgba8(r, g, b, byte(6).unwrap_or(255)),
            _ => Color::TRANSPARENT,
        },
        _ => Color::TRANSPARENT,
    }
}

pub fn rgba8(color: Color) -> [u8; 4] {
    let c = color.to_color_u8();
    [c.red(), c.green(), c.blue(), c.alpha()]
}

/// A paint source for `spec` over the rectangle (`x`, `y`, `w`, `h`): a plain
/// colour, or `grad(#from, #to[, angle])` — a linear gradient across the
/// rectangle (angle 0 = left to right, 90 = top to bottom).
pub fn shader(spec: &str, (x, y, w, h): (f32, f32, f32, f32)) -> Shader<'static> {
    let Some(inner) = spec.strip_prefix("grad(").and_then(|rest| rest.strip_suffix(')')) else {
        return Shader::SolidColor(parse(spec));
    };
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    if parts.len() < 2 {
        return Shader::SolidColor(Color::TRANSPARENT);
    }
    let angle = parts.get(2).and_then(|angle| angle.parse::<f32>().ok()).unwrap_or(0.0).to_radians();
    let (dx, dy) = (angle.cos(), angle.sin());
    let half = (w * dx.abs() + h * dy.abs()) / 2.0;
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    LinearGradient::new(
        Point::from_xy(cx - dx * half, cy - dy * half),
        Point::from_xy(cx + dx * half, cy + dy * half),
        vec![GradientStop::new(0.0, parse(parts[0])), GradientStop::new(1.0, parse(parts[1]))],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap_or(Shader::SolidColor(parse(parts[0])))
}
