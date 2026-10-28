//! Drawing a skin: its elements, in order, into a pixmap.

use tiny_skia::{Color, FillRule, LineCap, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

use crate::color::{parse, rgba8};
use crate::element::{Arc, Bar, Circle, Element, Hand, Rect, Skin, TextElement, Ticks};
use crate::text::{Style, Text};
use crate::vars::Vars;

/// Renders `skin` with `vars` at `scale`.
pub fn render(skin: &Skin, vars: &Vars, scale: f32, text: &mut Text) -> Pixmap {
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
        }
    }
    pixmap
}

fn paint(color: Color) -> Paint<'static> {
    let mut paint = Paint { anti_alias: true, ..Paint::default() };
    paint.set_color(color);
    paint
}

fn fill_and_stroke(pixmap: &mut Pixmap, path: &Path, fill: Option<&String>, stroke: Option<&String>, stroke_width: f32) {
    if let Some(fill) = fill {
        pixmap.fill_path(path, &paint(parse(fill)), FillRule::Winding, Transform::identity(), None);
    }
    if let Some(stroke) = stroke {
        pixmap.stroke_path(path, &paint(parse(stroke)), &Stroke { width: stroke_width, ..Stroke::default() }, Transform::identity(), None);
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
    let path = rounded_rect(rect.x * s + inset, rect.y * s + inset, rect.w * s - inset * 2.0, rect.h * s - inset * 2.0, rect.radius * s);
    if let Some(path) = path {
        fill_and_stroke(pixmap, &path, rect.fill.as_ref(), rect.stroke.as_ref(), rect.stroke_width * s);
    }
}

fn circle_shape(pixmap: &mut Pixmap, circle: &Circle, s: f32) {
    if let Some(path) = PathBuilder::from_circle(circle.cx * s, circle.cy * s, circle.r * s) {
        fill_and_stroke(pixmap, &path, circle.fill.as_ref(), circle.stroke.as_ref(), circle.stroke_width * s);
    }
}

fn draw_text(pixmap: &mut Pixmap, item: &TextElement, vars: &Vars, s: f32, text: &mut Text) {
    let content = vars.fill(&item.text);
    if content.is_empty() {
        return;
    }
    let style = Style { size: item.size * s, bold: item.bold, mono: item.mono, serif: item.serif, color: rgba8(parse(&item.color)) };
    let (width, _) = text.measure(&content, &style);
    let x = match item.align.as_str() {
        "center" => item.x * s - width / 2.0,
        "right" => item.x * s - width,
        _ => item.x * s,
    };
    text.draw(pixmap, &content, (x, item.y * s), &style);
}

fn bar_shape(pixmap: &mut Pixmap, bar: &Bar, vars: &Vars, s: f32) {
    let (x, y, w, h) = (bar.x * s, bar.y * s, bar.w * s, bar.h * s);
    if let Some(track) = rounded_rect(x, y, w, h, h / 2.0) {
        pixmap.fill_path(&track, &paint(parse(&bar.track)), FillRule::Winding, Transform::identity(), None);
    }
    let fraction = vars.get_number(&bar.value).clamp(0.0, 1.0);
    if fraction > 0.0 {
        // Never narrower than the bar is tall, so the rounded ends stay round.
        let filled = (w * fraction).max(h);
        if let Some(fill) = rounded_rect(x, y, filled.min(w), h, h / 2.0) {
            pixmap.fill_path(&fill, &paint(parse(&bar.fill)), FillRule::Winding, Transform::identity(), None);
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
    if let Some(track) = arc_path(cx, cy, r, arc.start, arc.sweep) {
        pixmap.stroke_path(&track, &paint(parse(&arc.track)), &stroke, Transform::identity(), None);
    }
    if let Some(name) = &arc.value {
        let fraction = vars.get_number(name).clamp(0.0, 1.0);
        if fraction > 0.0 {
            if let Some(fill) = arc_path(cx, cy, r, arc.start, arc.sweep * fraction) {
                pixmap.stroke_path(&fill, &paint(parse(&arc.fill)), &stroke, Transform::identity(), None);
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
        pixmap.stroke_path(&path, &paint(parse(&hand.color)), &stroke, Transform::identity(), None);
    }
}

fn ticks_shape(pixmap: &mut Pixmap, ticks: &Ticks, s: f32) {
    let stroke = Stroke { width: ticks.width * s, line_cap: LineCap::Round, ..Stroke::default() };
    let color = paint(parse(&ticks.color));
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
