//! Turning one Rainmeter skin (an `.ini`, already read) into a Paperloom look.
//!
//! Measures become the data Paperloom's widgets already offer (the time in any
//! `strftime` format, processor, memory, disk, network, battery, uptime);
//! meters become look elements (String → text, Image → image, Bar → bar,
//! Roundline → ring or clock hand, Shape → rectangles, circles and lines).
//! Whatever has no counterpart is left out and listed, so the user knows.

use std::collections::BTreeMap;
use std::f32::consts::PI;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

use super::ini::{number, Ini, Section};

/// What a measure becomes in a look.
#[derive(Debug, Clone)]
struct Measure {
    /// The widget kind whose data it needs (`None`: works in any).
    kind: Option<&'static str>,
    /// What `%1` turns into.
    text: String,
    /// The variable (0–1) that fills bars and rings.
    fraction: Option<String>,
    /// The variable (degrees) that turns a clock hand.
    angle: Option<String>,
    /// Its numbers already carry their unit (network speeds).
    has_unit: bool,
}

type Measures = BTreeMap<String, Result<Measure, String>>;

pub struct Converted {
    pub kind: String,
    pub size: (f32, f32),
    pub elements: Vec<Table>,
    /// Images to copy: from, and where in the look's folder.
    pub images: Vec<(PathBuf, String)>,
    /// What was left out, in words.
    pub notes: Vec<String>,
}

pub fn convert(ini: &Ini, skin_dir: &Path, root: &Path) -> Converted {
    let notes = Vec::new();
    // A measure Paperloom has no data for is only mentioned if a meter shows it.
    let mut measures: Measures = BTreeMap::new();
    for section in &ini.sections {
        if let Some(kind) = section.get("measure") {
            measures.insert(section.name.to_ascii_lowercase(), measure(section, kind));
        }
    }

    let mut out = Out { elements: Vec::new(), images: Vec::new(), notes, kinds: BTreeMap::new(), extent: (0.0, 0.0), prev: (0.0, 0.0, 0.0, 0.0), skin_dir, root };
    for section in &ini.sections {
        let Some(meter) = section.get("meter") else { continue };
        if section.get("hidden").and_then(number).is_some_and(|hidden| hidden != 0.0) {
            continue;
        }
        out.meter(section, meter, &measures);
    }

    // The background Rainmeter can paint behind the whole skin.
    let mut elements = out.elements;
    if let Some(rainmeter) = ini.section("Rainmeter") {
        if let Some(color) = rainmeter.get("solidcolor").and_then(color) {
            let mut rect = Table::new();
            rect.insert("type".into(), "rect".into());
            for (key, value) in [("x", 0.0), ("y", 0.0), ("w", out.extent.0), ("h", out.extent.1)] {
                rect.insert(key.into(), f(value));
            }
            rect.insert("fill".into(), color.into());
            elements.insert(0, rect);
        }
    }

    // The kind most of its meters need; the time works in any kind.
    let kind = out.kinds.iter().max_by_key(|(_, count)| **count).map_or("clock", |(kind, _)| *kind).to_string();
    let mut notes = out.notes;
    notes.sort();
    notes.dedup();
    Converted { kind, size: (out.extent.0.max(20.0).ceil(), out.extent.1.max(20.0).ceil()), elements, images: out.images, notes }
}

fn measure(section: &Section, kind: &str) -> Result<Measure, String> {
    let plain = |kind: &'static str, text: &str, fraction: Option<&str>| Measure { kind: Some(kind), text: text.into(), fraction: fraction.map(str::to_string), angle: None, has_unit: false };
    let percent = section.get("percentual").and_then(number).is_some_and(|on| on != 0.0);
    let measure = match kind.to_ascii_lowercase().as_str() {
        "time" => {
            // Rainmeter's "no leading zero" flag is `#`; chrono's is `-`.
            let format = section.get("format").unwrap_or("%H:%M:%S").replace("%#", "%-");
            let has = |codes: &[&str]| codes.iter().any(|code| format.contains(code));
            let (fraction, angle) = if has(&["%S"]) {
                (Some("second_fraction"), Some("second_angle"))
            } else if has(&["%M"]) {
                (Some("minute_fraction"), Some("minute_angle"))
            } else if has(&["%H", "%I", "%-H", "%-I"]) {
                (Some("hour_fraction"), Some("hour_angle"))
            } else {
                (None, None)
            };
            let clock = has(&["%H", "%I", "%M", "%S", "%T", "%R", "%X", "%p", "%-H", "%-I"]);
            Measure { kind: (clock || angle.is_some()).then_some("clock"), text: format!("{{{format}}}"), fraction: fraction.map(str::to_string), angle: angle.map(str::to_string), has_unit: false }
        }
        "cpu" => plain("cpu", "{percent}", Some("fraction")),
        "physicalmemory" | "memory" | "swapmemory" => plain("memory", if percent { "{percent}" } else { "{used}" }, Some("fraction")),
        "freediskspace" => plain("disk", if percent { "{percent}" } else { "{free}" }, Some("fraction")),
        "netin" => Measure { has_unit: true, ..plain("network", "{down}", None) },
        "netout" => Measure { has_unit: true, ..plain("network", "{up}", None) },
        "nettotal" => Measure { has_unit: true, ..plain("network", "{value}", None) },
        "uptime" => plain("uptime", "{value}", None),
        "string" => Measure { kind: None, text: section.get("string").unwrap_or("").to_string(), fraction: None, angle: None, has_unit: false },
        "plugin" => {
            let plugin = section.get("plugin").unwrap_or("").to_ascii_lowercase();
            if plugin.contains("powerplugin") && section.get("powerstate").is_some_and(|state| state.eq_ignore_ascii_case("percent")) {
                plain("battery", "{percent}", Some("fraction"))
            } else {
                let name = plugin.rsplit(['\\', '/']).next().unwrap_or(&plugin).trim_end_matches(".dll").to_string();
                return Err(format!("{}: the {name} plugin", section.name));
            }
        }
        other => return Err(format!("{}: {other} data", section.name)),
    };
    Ok(measure)
}

struct Out<'a> {
    elements: Vec<Table>,
    images: Vec<(PathBuf, String)>,
    notes: Vec<String>,
    kinds: BTreeMap<&'static str, u32>,
    /// The far right and bottom edges so far: the look's size.
    extent: (f32, f32),
    /// The previous meter's x, y, width and height (for `r` and `R` positions).
    prev: (f32, f32, f32, f32),
    skin_dir: &'a Path,
    root: &'a Path,
}

impl Out<'_> {
    fn meter(&mut self, section: &Section, meter: &str, measures: &Measures) {
        // The measures it shows, in order (MeasureName, MeasureName2, …).
        let mut used: Vec<Option<&Measure>> = Vec::new();
        for key in std::iter::once("measurename".to_string()).chain((2..=9).map(|index| format!("measurename{index}"))) {
            let Some(name) = section.get(&key) else { break };
            let found = match measures.get(&name.to_ascii_lowercase()) {
                Some(Ok(found)) => Some(found),
                Some(Err(note)) => {
                    self.notes.push(note.clone());
                    None
                }
                None => None,
            };
            if let Some(kind) = found.and_then(|found| found.kind) {
                *self.kinds.entry(kind).or_default() += 1;
            }
            used.push(found);
        }
        let first = used.first().copied().flatten();

        let x = position(section.get("x"), self.prev.0, self.prev.2);
        let y = position(section.get("y"), self.prev.1, self.prev.3);
        let mut w = section.get("w").and_then(number).unwrap_or(0.0);
        let mut h = section.get("h").and_then(number).unwrap_or(0.0);

        match meter.to_ascii_lowercase().as_str() {
            "string" => {
                let size = section.get("fontsize").and_then(number).unwrap_or(10.0) * 4.0 / 3.0;
                let mut content = section.get("text").map_or_else(|| if used.is_empty() { String::new() } else { "%1".into() }, str::to_string);
                for (index, found) in used.iter().enumerate().rev() {
                    content = content.replace(&format!("%{}", index + 1), found.map_or("", |found| found.text.as_str()));
                }
                // `[MeasureName]` inline references.
                for (name, found) in measures {
                    let pattern = format!("[{name}]");
                    while let Some(at) = content.to_ascii_lowercase().find(&pattern) {
                        let with = match found {
                            Ok(found) => found.text.as_str(),
                            Err(note) => {
                                self.notes.push(note.clone());
                                ""
                            }
                        };
                        content.replace_range(at..at + pattern.len(), with);
                    }
                }
                let postfix = section.get("postfix").unwrap_or("");
                let postfix = if first.is_some_and(|found| found.has_unit) { "" } else { postfix };
                content = format!("{}{content}{postfix}", section.get("prefix").unwrap_or(""));
                let align = section.get("stringalign").unwrap_or("left").to_ascii_lowercase();
                let across = if align.starts_with("center") { "center" } else if align.starts_with("right") { "right" } else { "left" };
                // Vertical alignment: our text is placed by its top.
                let line = size * 1.25;
                let top = if align.ends_with("bottom") { y - line } else if align.len() > 6 && align.ends_with("center") { y - line / 2.0 } else { y };
                if w == 0.0 {
                    w = content.chars().filter(|c| *c != '{' && *c != '}').count().min(24) as f32 * size * 0.55;
                }
                if h == 0.0 {
                    h = line;
                }
                let left = match across {
                    "center" => x - w / 2.0,
                    "right" => x - w,
                    _ => x,
                };
                if let Some(fill) = section.get("solidcolor").and_then(color) {
                    self.push(rect(left, top, w, h, Some(fill), None, 0.0, 0.0));
                }
                let mut text = Table::new();
                text.insert("type".into(), "text".into());
                text.insert("text".into(), content.into());
                text.insert("x".into(), f(x));
                text.insert("y".into(), f(top));
                text.insert("size".into(), f(size));
                text.insert("align".into(), across.into());
                text.insert("color".into(), section.get("fontcolor").and_then(color).unwrap_or_else(|| "#000000ff".into()).into());
                if section.get("stringstyle").is_some_and(|style| style.to_ascii_lowercase().contains("bold")) {
                    text.insert("bold".into(), true.into());
                }
                if let Some(face) = section.get("fontface") {
                    text.insert("font".into(), face.into());
                }
                if let Some(case) = section.get("stringcase").map(str::to_ascii_lowercase).filter(|case| case == "upper" || case == "lower") {
                    text.insert("case".into(), case.into());
                }
                if section.get("stringeffect").is_some_and(|effect| !effect.eq_ignore_ascii_case("none")) {
                    text.insert("shadow".into(), section.get("fonteffectcolor").and_then(color).unwrap_or_else(|| "#00000080".into()).into());
                }
                if section.get("inlinesetting").is_some() {
                    self.notes.push(format!("{}: inline text styles", section.name));
                }
                self.extent_of(left, top, w, h);
                self.elements.push(text);
            }
            "image" => {
                let Some(name) = section.get("imagename") else {
                    if let Some(fill) = section.get("solidcolor").and_then(color) {
                        self.push(rect(x, y, w, h, Some(fill), None, 0.0, 0.0));
                        self.extent_of(x, y, w, h);
                    }
                    self.prev = (x, y, w, h);
                    return;
                };
                match self.image(section, name) {
                    Some((dest, natural)) => {
                        // A missing side keeps the picture's proportions.
                        match (w > 0.0, h > 0.0) {
                            (false, false) => (w, h) = natural,
                            (true, false) => h = w * natural.1 / natural.0.max(1.0),
                            (false, true) => w = h * natural.0 / natural.1.max(1.0),
                            _ => {}
                        }
                        let mut image = Table::new();
                        image.insert("type".into(), "image".into());
                        image.insert("src".into(), dest.into());
                        for (key, value) in [("x", x), ("y", y), ("w", w), ("h", h)] {
                            image.insert(key.into(), f(value));
                        }
                        if let Some(alpha) = section.get("imagealpha").and_then(number) {
                            image.insert("opacity".into(), f(alpha / 255.0));
                        }
                        if section.get("imagetint").is_some() {
                            self.notes.push(format!("{}: tinted images", section.name));
                        }
                        self.extent_of(x, y, w, h);
                        self.elements.push(image);
                    }
                    None => self.notes.push(format!("{}: the picture {name}", section.name)),
                }
            }
            "bar" => {
                if section.get("barorientation").is_some_and(|way| way.eq_ignore_ascii_case("vertical")) {
                    self.notes.push(format!("{}: an upright bar", section.name));
                } else if let Some(value) = first.and_then(|found| found.fraction.clone()) {
                    let mut bar = Table::new();
                    bar.insert("type".into(), "bar".into());
                    for (key, number) in [("x", x), ("y", y), ("w", w), ("h", h)] {
                        bar.insert(key.into(), f(number));
                    }
                    bar.insert("value".into(), value.into());
                    bar.insert("fill".into(), section.get("barcolor").and_then(color).unwrap_or_else(|| "#00ff00ff".into()).into());
                    bar.insert("track".into(), section.get("solidcolor").and_then(color).unwrap_or_else(|| "#00000000".into()).into());
                    self.extent_of(x, y, w, h);
                    self.elements.push(bar);
                } else {
                    self.notes.push(format!("{}: a bar without data it can show", section.name));
                }
            }
            "roundline" => {
                let outer = section.get("linelength").and_then(number).unwrap_or(w.min(h) / 2.0);
                let inner = section.get("linestart").and_then(number).unwrap_or(0.0);
                let (cx, cy) = (x + w / 2.0, y + h / 2.0);
                // Rainmeter: radians from three o'clock; Paperloom: degrees from twelve.
                let start = section.get("startangle").and_then(number).unwrap_or(0.0) * 180.0 / PI + 90.0;
                let sweep = section.get("rotationangle").and_then(number).unwrap_or(2.0 * PI) * 180.0 / PI;
                let line = section.get("linecolor").and_then(color).unwrap_or_else(|| "#000000ff".into());
                let solid = section.get("solid").and_then(number).is_some_and(|on| on != 0.0);
                if solid {
                    let mut arc = Table::new();
                    arc.insert("type".into(), "arc".into());
                    arc.insert("cx".into(), f(cx));
                    arc.insert("cy".into(), f(cy));
                    arc.insert("r".into(), f((outer + inner) / 2.0));
                    arc.insert("width".into(), f((outer - inner).abs().max(1.0)));
                    arc.insert("start".into(), f(if sweep < 0.0 { start + sweep } else { start }));
                    arc.insert("sweep".into(), f(sweep.abs()));
                    if let Some(value) = first.and_then(|found| found.fraction.clone()) {
                        arc.insert("value".into(), value.into());
                    }
                    arc.insert("fill".into(), line.into());
                    arc.insert("track".into(), "#00000000".into());
                    self.elements.push(arc);
                } else if let Some(angle) = first.and_then(|found| found.angle.clone()) {
                    let mut hand = Table::new();
                    hand.insert("type".into(), "hand".into());
                    hand.insert("cx".into(), f(cx));
                    hand.insert("cy".into(), f(cy));
                    hand.insert("length".into(), f(outer));
                    hand.insert("width".into(), f(section.get("linewidth").and_then(number).unwrap_or(1.0)));
                    hand.insert("angle".into(), angle.into());
                    hand.insert("color".into(), line.into());
                    self.elements.push(hand);
                } else {
                    self.notes.push(format!("{}: a dial needle for data other than the time", section.name));
                }
                self.extent_of(cx - outer, cy - outer, outer * 2.0, outer * 2.0);
            }
            "shape" => {
                for key in std::iter::once("shape".to_string()).chain((2..=16).map(|index| format!("shape{index}"))) {
                    let Some(spec) = section.get(&key) else { continue };
                    match shape(spec, x, y) {
                        Some((element, bounds)) => {
                            self.extent_of(bounds.0, bounds.1, bounds.2, bounds.3);
                            if w == 0.0 && h == 0.0 {
                                (w, h) = (bounds.0 + bounds.2 - x, bounds.1 + bounds.3 - y);
                            }
                            self.push(element);
                        }
                        None => self.notes.push(format!("{}: a {} shape", section.name, spec.split_whitespace().next().unwrap_or("custom").to_ascii_lowercase())),
                    }
                }
            }
            other => self.notes.push(format!("{}: {other} meters", section.name)),
        }
        self.prev = (x, y, w, h);
    }

    fn push(&mut self, element: Table) {
        let empty = |key: &str| element.get(key).and_then(Value::as_float).is_some_and(|value| value <= 0.0);
        if empty("w") || empty("h") {
            return;
        }
        self.elements.push(element);
    }

    fn extent_of(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.extent.0 = self.extent.0.max(x + w);
        self.extent.1 = self.extent.1.max(y + h);
    }

    /// Finds a meter's picture, queues it to be copied, and gives its new name and size.
    fn image(&mut self, section: &Section, name: &str) -> Option<(String, (f32, f32))> {
        let name = name.replace('\\', "/");
        let base = section.get("imagepath").map(|path| PathBuf::from(path.replace('\\', "/"))).unwrap_or_else(|| self.skin_dir.to_path_buf());
        let path = if Path::new(&name).is_absolute() { PathBuf::from(&name) } else { base.join(&name) };
        let path = if path.extension().is_none() { path.with_extension("png") } else { path };
        let canonical = path.canonicalize().ok()?;
        if !canonical.starts_with(self.root.canonicalize().ok()?) || !path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("png")) {
            return None;
        }
        let pixmap = tiny_skia::Pixmap::decode_png(&std::fs::read(&canonical).ok()?).ok()?;
        let dest = match self.images.iter().find(|(from, _)| *from == canonical) {
            Some((_, dest)) => dest.clone(),
            None => {
                let dest = format!("images/{}.png", self.images.len() + 1);
                self.images.push((canonical, dest.clone()));
                dest
            }
        };
        Some((dest, (pixmap.width() as f32, pixmap.height() as f32)))
    }
}

/// `10`, `10r` (from the previous meter's position) or `10R` (from its far edge).
fn position(value: Option<&str>, prev: f32, prev_size: f32) -> f32 {
    let Some(value) = value.map(str::trim) else { return 0.0 };
    if let Some(offset) = value.strip_suffix('r') {
        prev + number(offset).unwrap_or(0.0)
    } else if let Some(offset) = value.strip_suffix('R') {
        prev + prev_size + number(offset).unwrap_or(0.0)
    } else {
        number(value).unwrap_or(0.0)
    }
}

/// `255,255,255,200` or `FFFFFFC8` → `#ffffffc8`.
fn color(spec: &str) -> Option<String> {
    let spec = spec.trim();
    if spec.contains(',') {
        let parts: Vec<f32> = spec.split(',').map(|part| number(part).unwrap_or(0.0)).collect();
        let channel = |index: usize, default: f32| parts.get(index).copied().unwrap_or(default).clamp(0.0, 255.0) as u8;
        return Some(format!("#{:02x}{:02x}{:02x}{:02x}", channel(0, 0.0), channel(1, 0.0), channel(2, 0.0), channel(3, 255.0)));
    }
    let hex = spec.trim_start_matches('#');
    (matches!(hex.len(), 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit())).then(|| format!("#{}", hex.to_ascii_lowercase()))
}

fn f(value: f32) -> Value {
    Value::Float(f64::from((value * 100.0).round() / 100.0))
}

#[allow(clippy::too_many_arguments)]
fn rect(x: f32, y: f32, w: f32, h: f32, fill: Option<String>, stroke: Option<String>, stroke_width: f32, radius: f32) -> Table {
    let mut rect = Table::new();
    rect.insert("type".into(), "rect".into());
    for (key, value) in [("x", x), ("y", y), ("w", w), ("h", h), ("radius", radius)] {
        rect.insert(key.into(), f(value));
    }
    if let Some(fill) = fill {
        rect.insert("fill".into(), fill.into());
    }
    if let Some(stroke) = stroke.filter(|_| stroke_width > 0.0) {
        rect.insert("stroke".into(), stroke.into());
        rect.insert("stroke_width".into(), f(stroke_width));
    }
    rect
}

/// `Rectangle 0,0,200,80,8 | Fill Color 0,0,0,150 | StrokeWidth 0`, placed at (`x`, `y`).
fn shape(spec: &str, x: f32, y: f32) -> Option<(Table, (f32, f32, f32, f32))> {
    let mut parts = spec.split('|').map(str::trim);
    let head = parts.next()?;
    let (form, args) = head.split_once(char::is_whitespace)?;
    let args: Vec<f32> = args.split(',').map(|arg| number(arg).unwrap_or(0.0)).collect();
    let (mut fill, mut stroke, mut stroke_width) = (Some("#ffffffff".to_string()), Some("#000000ff".to_string()), 1.0);
    for modifier in parts {
        let lower = modifier.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("fill color") {
            fill = color(rest);
        } else if let Some(rest) = lower.strip_prefix("stroke color") {
            stroke = color(rest);
        } else if let Some(rest) = lower.strip_prefix("strokewidth") {
            stroke_width = number(rest).unwrap_or(1.0);
        }
    }
    let arg = |index: usize| args.get(index).copied().unwrap_or(0.0);
    match form.to_ascii_lowercase().as_str() {
        "rectangle" => {
            let (rx, ry, w, h) = (x + arg(0), y + arg(1), arg(2), arg(3));
            Some((rect(rx, ry, w, h, fill, stroke, stroke_width, arg(4)), (rx, ry, w, h)))
        }
        "ellipse" if args.len() < 4 || (arg(2) - arg(3)).abs() < 0.5 => {
            let (cx, cy, r) = (x + arg(0), y + arg(1), arg(2));
            let mut circle = Table::new();
            circle.insert("type".into(), "circle".into());
            circle.insert("cx".into(), f(cx));
            circle.insert("cy".into(), f(cy));
            circle.insert("r".into(), f(r));
            if let Some(fill) = fill {
                circle.insert("fill".into(), fill.into());
            }
            if let Some(stroke) = stroke.filter(|_| stroke_width > 0.0) {
                circle.insert("stroke".into(), stroke.into());
                circle.insert("stroke_width".into(), f(stroke_width));
            }
            Some((circle, (cx - r, cy - r, r * 2.0, r * 2.0)))
        }
        "line" => {
            let mut line = Table::new();
            line.insert("type".into(), "line".into());
            for (key, value) in [("x1", x + arg(0)), ("y1", y + arg(1)), ("x2", x + arg(2)), ("y2", y + arg(3)), ("width", stroke_width)] {
                line.insert(key.into(), f(value));
            }
            line.insert("color".into(), stroke.unwrap_or_else(|| "#000000ff".into()).into());
            let (left, top) = ((x + arg(0)).min(x + arg(2)), (y + arg(1)).min(y + arg(3)));
            Some((line, (left, top, (arg(2) - arg(0)).abs(), (arg(3) - arg(1)).abs())))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors() {
        assert_eq!(color("255,255,255,200").as_deref(), Some("#ffffffc8"));
        assert_eq!(color("255,0,0").as_deref(), Some("#ff0000ff"));
        assert_eq!(color("FFFFFF80").as_deref(), Some("#ffffff80"));
        assert_eq!(color("nope"), None);
    }

    #[test]
    fn positions() {
        assert_eq!(position(Some("10r"), 5.0, 20.0), 15.0);
        assert_eq!(position(Some("10R"), 5.0, 20.0), 35.0);
        assert_eq!(position(Some("(4*2)"), 5.0, 20.0), 8.0);
    }
}
