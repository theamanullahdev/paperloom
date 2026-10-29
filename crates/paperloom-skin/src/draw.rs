//! Drawing a skin: its elements, in order, into a pixmap.

use tiny_skia::{FillRule, LineCap, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Rect as SkRect, Shader, Stroke, Transform};

use crate::color::{parse, rgba8, shader};
use crate::element::{Arc, Bar, Circle, Element, Hand, Image, Line, Rect, Segments, Skin, TextElement, Ticks};
use crate::text::{Style, Text};
use crate::vars::Vars;

/// Renders `skin` with `vars` at `scale`.
pub fn render(skin: &Skin, vars: &Vars, scale: f32, text: &mut Text) -> Pixmap {
    text.ensure_fonts(skin);
    let width = ((skin.size[0] * scale) as u32).max(1);
    let height = ((skin.size[1] * scale) as u32).max(1);
    let mut pixmap = Pixmap::new(width, height).expect("non-empty pixmap");
    for element in &skin.element {
        if element.show_if().is_some_and(|name| !vars.has(name)) {
            continue;
        }
        match element {
            Element::Rect(rect) => rectangle(&mut pixmap, rect, scale),
            Element::Circle(circle) => circle_shape(&mut pixmap, circle, scale),
            Element::Text(item) => draw_text(&mut pixmap, item, vars, scale, text),
            Element::Bar(bar) => bar_shape(&mut pixmap, bar, vars, scale),
            Element::Arc(arc) => arc_shape(&mut pixmap, arc, vars, scale),
            Element::Hand(hand) => hand_shape(&mut pixmap, hand, vars, scale),
            Element::Ticks(ticks) => ticks_shape(&mut pixmap, ticks, scale),
            Element::Line(line) => line_shape(&mut pixmap, line, scale),
            Element::Image(image) => image_shape(&mut pixmap, image, skin, scale, text),
            Element::Segments(segments) => segments_shape(&mut pixmap, segments, vars, scale),
        }
    }
    pixmap
}

type Bounds = (f32, f32, f32, f32);

fn paint(spec: &str, bounds: Bounds) -> Paint<'static> {
    Paint { anti_alias: true, shader: shader(spec, bounds), ..Paint::default() }
}

/// The same paint, fainter (for the layers of a glow).
fn faded(spec: &str, bounds: Bounds, opacity: f32) -> Paint<'static> {
    let mut paint = paint(spec, bounds);
    if let Shader::SolidColor(_) | Shader::LinearGradient(_) = &paint.shader {
        paint.shader.apply_opacity(opacity);
    }
    paint
}

/// Strokes `path`, with a soft halo first when `glow` asks for one.
fn stroke_glowing(pixmap: &mut Pixmap, path: &Path, spec: &str, bounds: Bounds, stroke: &Stroke, glow: Option<f32>) {
    if let Some(glow) = glow.filter(|glow| *glow > 0.0) {
        for (grow, opacity) in [(1.0 + glow, 0.10), (1.0 + glow * 0.6, 0.16), (1.0 + glow * 0.3, 0.24)] {
            let halo = Stroke { width: stroke.width * grow, ..stroke.clone() };
            pixmap.stroke_path(path, &faded(spec, bounds, opacity), &halo, Transform::identity(), None);
        }
    }
    pixmap.stroke_path(path, &paint(spec, bounds), stroke, Transform::identity(), None);
}

fn fill_and_stroke(pixmap: &mut Pixmap, path: &Path, bounds: Bounds, fill: Option<&String>, stroke: Option<&String>, stroke_width: f32, glow: Option<f32>) {
    if let Some(fill) = fill {
        pixmap.fill_path(path, &paint(fill, bounds), FillRule::Winding, Transform::identity(), None);
    }
    if let Some(color) = stroke {
        stroke_glowing(pixmap, path, color, bounds, &Stroke { width: stroke_width, ..Stroke::default() }, glow);
    }
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, radius: f32) -> Option<Path> {
    let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
    let mut b = PathBuilder::new();
    b.move_to(x + r, y);
    b.line_to(x + w - r, y);
    b.quad_to(x + w, y, x + w, y + r);
    b.line_to(x + w, y + h - r);
    b.quad_to(x + w, y + h, x + w - r, y + h);
    b.line_to(x + r, y + h);
    b.quad_to(x, y + h, x, y + h - r);
    b.line_to(x, y + r);
    b.quad_to(x, y, x + r, y);
    b.close();
    b.finish()
}

fn rectangle(pixmap: &mut Pixmap, rect: &Rect, s: f32) {
    // Strokes sit inside the edge: inset by half the line.
    let inset = if rect.stroke.is_some() { rect.stroke_width * s / 2.0 } else { 0.0 };
    let bounds = (rect.x * s, rect.y * s, rect.w * s, rect.h * s);
    if let Some(path) = rounded_rect(bounds.0 + inset, bounds.1 + inset, bounds.2 - inset * 2.0, bounds.3 - inset * 2.0, rect.radius * s) {
        fill_and_stroke(pixmap, &path, bounds, rect.fill.as_ref(), rect.stroke.as_ref(), rect.stroke_width * s, rect.glow);
    }
}

fn circle_shape(pixmap: &mut Pixmap, circle: &Circle, s: f32) {
    let (cx, cy, r) = (circle.cx * s, circle.cy * s, circle.r * s);
    if let Some(path) = PathBuilder::from_circle(cx, cy, r) {
        fill_and_stroke(pixmap, &path, (cx - r, cy - r, r * 2.0, r * 2.0), circle.fill.as_ref(), circle.stroke.as_ref(), circle.stroke_width * s, None);
    }
}

fn draw_text(pixmap: &mut Pixmap, item: &TextElement, vars: &Vars, s: f32, text: &mut Text) {
    let content = match item.case.as_str() {
        "upper" => vars.fill(&item.text).to_uppercase(),
        "lower" => vars.fill(&item.text).to_lowercase(),
        _ => vars.fill(&item.text),
    };
    if content.is_empty() {
        return;
    }
    let mut style = Style { size: item.size * s, bold: item.bold, mono: item.mono, serif: item.serif, font: item.font.clone(), color: rgba8(parse(&item.color)) };
    let (width, _) = text.measure(&content, &style);
    let x = match item.align.as_str() {
        "center" => item.x * s - width / 2.0,
        "right" => item.x * s - width,
        _ => item.x * s,
    };
    if let Some(shadow) = &item.shadow {
        let offset = (item.size / 24.0).clamp(1.0, 3.0) * s;
        let shadow_style = Style { color: rgba8(parse(shadow)), ..style.clone() };
        text.draw(pixmap, &content, (x + offset, item.y * s + offset), &shadow_style);
    }
    style.color = rgba8(parse(&item.color));
    text.draw(pixmap, &content, (x, item.y * s), &style);
}

fn bar_shape(pixmap: &mut Pixmap, bar: &Bar, vars: &Vars, s: f32) {
    let (x, y, w, h) = (bar.x * s, bar.y * s, bar.w * s, bar.h * s);
    if let Some(track) = rounded_rect(x, y, w, h, h / 2.0) {
        pixmap.fill_path(&track, &paint(&bar.track, (x, y, w, h)), FillRule::Winding, Transform::identity(), None);
    }
    let fraction = vars.get_number(&bar.value).clamp(0.0, 1.0);
    if fraction > 0.0 {
        // Never narrower than the bar is tall, so the rounded ends stay round.
        let filled = (w * fraction).max(h).min(w);
        if let Some(fill) = rounded_rect(x, y, filled, h, h / 2.0) {
            if let Some(glow) = bar.glow.filter(|glow| *glow > 0.0) {
                for (grow, opacity) in [(glow, 0.12), (glow * 0.5, 0.2)] {
                    if let Some(halo) = rounded_rect(x - grow, y - grow, filled + grow * 2.0, h + grow * 2.0, h / 2.0 + grow) {
                        pixmap.fill_path(&halo, &faded(&bar.fill, (x, y, w, h), opacity), FillRule::Winding, Transform::identity(), None);
                    }
                }
            }
            pixmap.fill_path(&fill, &paint(&bar.fill, (x, y, w, h)), FillRule::Winding, Transform::identity(), None);
        }
    }
}

/// A point on a circle: `angle` degrees clockwise from twelve o'clock.
fn on_circle(cx: f32, cy: f32, radius: f32, angle: f32) -> (f32, f32) {
    let radians = angle.to_radians();
    (cx + radius * radians.sin(), cy - radius * radians.cos())
}

fn arc_path(cx: f32, cy: f32, r: f32, start: f32, sweep: f32) -> Option<Path> {
    let steps = ((sweep.abs() / 4.0).ceil() as usize).max(2);
    let mut b = PathBuilder::new();
    for step in 0..=steps {
        let (x, y) = on_circle(cx, cy, r, start + sweep * step as f32 / steps as f32);
        if step == 0 {
            b.move_to(x, y);
        } else {
            b.line_to(x, y);
        }
    }
    b.finish()
}

fn arc_shape(pixmap: &mut Pixmap, arc: &Arc, vars: &Vars, s: f32) {
    let stroke = Stroke { width: arc.width * s, line_cap: LineCap::Round, ..Stroke::default() };
    let (cx, cy, r) = (arc.cx * s, arc.cy * s, arc.r * s);
    let bounds = (cx - r, cy - r, r * 2.0, r * 2.0);
    if let Some(track) = arc_path(cx, cy, r, arc.start, arc.sweep) {
        pixmap.stroke_path(&track, &paint(&arc.track, bounds), &stroke, Transform::identity(), None);
    }
    if let Some(name) = &arc.value {
        let fraction = vars.get_number(name).clamp(0.0, 1.0);
        if fraction > 0.0 {
            if let Some(fill) = arc_path(cx, cy, r, arc.start, arc.sweep * fraction) {
                stroke_glowing(pixmap, &fill, &arc.fill, bounds, &stroke, arc.glow);
            }
        }
    }
}

fn hand_shape(pixmap: &mut Pixmap, hand: &Hand, vars: &Vars, s: f32) {
    let angle = vars.get_number(&hand.angle);
    let (cx, cy) = (hand.cx * s, hand.cy * s);
    let (tip_x, tip_y) = on_circle(cx, cy, hand.length * s, angle);
    let (tail_x, tail_y) = if hand.tail > 0.0 { on_circle(cx, cy, hand.tail * s, angle + 180.0) } else { (cx, cy) };
    let mut b = PathBuilder::new();
    b.move_to(tail_x, tail_y);
    b.line_to(tip_x, tip_y);
    if let Some(path) = b.finish() {
        let stroke = Stroke { width: hand.width * s, line_cap: LineCap::Round, ..Stroke::default() };
        let reach = hand.length * s;
        stroke_glowing(pixmap, &path, &hand.color, (cx - reach, cy - reach, reach * 2.0, reach * 2.0), &stroke, hand.glow);
    }
}

fn ticks_shape(pixmap: &mut Pixmap, ticks: &Ticks, s: f32) {
    let stroke = Stroke { width: ticks.width * s, line_cap: LineCap::Round, ..Stroke::default() };
    let reach = ticks.outer * s;
    let color = paint(&ticks.color, (ticks.cx * s - reach, ticks.cy * s - reach, reach * 2.0, reach * 2.0));
    for index in 0..ticks.count {
        let angle = 360.0 * index as f32 / ticks.count.max(1) as f32;
        let big = ticks.big_every > 0 && index.is_multiple_of(ticks.big_every);
        let inner = if big { ticks.big_inner.unwrap_or(ticks.inner) } else { ticks.inner };
        let (x1, y1) = on_circle(ticks.cx * s, ticks.cy * s, ticks.outer * s, angle);
        let (x2, y2) = on_circle(ticks.cx * s, ticks.cy * s, inner * s, angle);
        let mut b = PathBuilder::new();
        b.move_to(x1, y1);
        b.line_to(x2, y2);
        if let Some(path) = b.finish() {
            pixmap.stroke_path(&path, &color, &stroke, Transform::identity(), None);
        }
    }
}

fn line_shape(pixmap: &mut Pixmap, line: &Line, s: f32) {
    let mut b = PathBuilder::new();
    b.move_to(line.x1 * s, line.y1 * s);
    b.line_to(line.x2 * s, line.y2 * s);
    if let Some(path) = b.finish() {
        let (left, top) = (line.x1.min(line.x2) * s, line.y1.min(line.y2) * s);
        let bounds = (left, top, ((line.x1 - line.x2).abs() * s).max(1.0), ((line.y1 - line.y2).abs() * s).max(1.0));
        let stroke = Stroke { width: line.width * s, line_cap: LineCap::Round, ..Stroke::default() };
        stroke_glowing(pixmap, &path, &line.color, bounds, &stroke, line.glow);
    }
}

fn image_shape(pixmap: &mut Pixmap, image: &Image, skin: &Skin, s: f32, text: &mut Text) {
    let Some(dir) = &skin.dir else { return };
    // A look can only reach files inside its own folder.
    if image.src.contains("..") || image.src.starts_with('/') || image.src.contains(':') {
        return;
    }
    let Some(source) = text.image(&dir.join(&image.src)) else { return };
    let (sx, sy) = (image.w * s / source.width() as f32, image.h * s / source.height() as f32);
    let paint = PixmapPaint { opacity: image.opacity.clamp(0.0, 1.0), quality: tiny_skia::FilterQuality::Bilinear, ..PixmapPaint::default() };
    pixmap.draw_pixmap(0, 0, source.as_ref(), &paint, Transform::from_row(sx, 0.0, 0.0, sy, image.x * s, image.y * s), None);
}

fn segments_shape(pixmap: &mut Pixmap, seg: &Segments, vars: &Vars, s: f32) {
    let fraction = vars.get_number(&seg.value).clamp(0.0, 1.0);
    let count = seg.count.max(1);
    let lit_for = |index: u32| fraction * count as f32 > index as f32 + 0.5 || (fraction >= 1.0 && index < count);
    if seg.r > 0.0 {
        let (cx, cy, r) = (seg.cx * s, seg.cy * s, seg.r * s);
        let bounds = (cx - r, cy - r, r * 2.0, r * 2.0);
        let each = seg.sweep / count as f32;
        let stroke = Stroke { width: seg.width * s, line_cap: LineCap::Butt, ..Stroke::default() };
        for index in 0..count {
            let from = seg.start + each * index as f32 + seg.gap / 2.0;
            let Some(path) = arc_path(cx, cy, r, from, (each - seg.gap).max(0.5)) else { continue };
            if lit_for(index) {
                stroke_glowing(pixmap, &path, &seg.fill, bounds, &stroke, seg.glow);
            } else {
                pixmap.stroke_path(&path, &paint(&seg.track, bounds), &stroke, Transform::identity(), None);
            }
        }
    } else {
        let (x, y, w, h, gap) = (seg.x * s, seg.y * s, seg.w * s, seg.h * s, seg.gap * s);
        let each = ((w - gap * (count - 1) as f32) / count as f32).max(1.0);
        for index in 0..count {
            let left = x + (each + gap) * index as f32;
            let Some(rect) = SkRect::from_xywh(left, y, each, h) else { continue };
            let path = PathBuilder::from_rect(rect);
            let spec = if lit_for(index) { &seg.fill } else { &seg.track };
            pixmap.fill_path(&path, &paint(spec, (x, y, w, h)), FillRule::Winding, Transform::identity(), None);
        }
    }
}

