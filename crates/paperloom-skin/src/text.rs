//! Text into a pixmap. One `Text` owns the font database and glyph cache;
//! shaping (cosmic-text) means right-to-left and complex scripts render
//! correctly.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache, Weight};
use tiny_skia::Pixmap;

use crate::element::Skin;

pub struct Text {
    fonts: FontSystem,
    cache: SwashCache,
    /// Font files already added (each once), and images already decoded.
    loaded_fonts: HashSet<PathBuf>,
    images: HashMap<PathBuf, Option<Pixmap>>,
    /// Text already laid out, by its words and style (laying out is the
    /// costly part of drawing a widget; most of its text never changes).
    layouts: HashMap<LayoutKey, Buffer>,
    /// Each look's unchanging part, by look and scale.
    bases: HashMap<(usize, u32), Pixmap>,
}

type LayoutKey = (String, u32, bool, bool, bool, Option<String>);

fn layout_key(text: &str, style: &Style) -> LayoutKey {
    (text.to_string(), style.size.to_bits(), style.bold, style.mono, style.serif, style.font.clone())
}

/// How a piece of text looks.
#[derive(Debug, Clone)]
pub struct Style {
    pub size: f32,
    pub bold: bool,
    pub mono: bool,
    pub serif: bool,
    /// A family name from a font the look ships.
    pub font: Option<String>,
    pub color: [u8; 4],
}

impl Style {
    pub fn plain(size: f32, color: [u8; 4]) -> Self {
        Self { size, bold: false, mono: false, serif: false, font: None, color }
    }

    pub fn bold(size: f32, color: [u8; 4]) -> Self {
        Self { bold: true, ..Self::plain(size, color) }
    }
}

impl Text {
    pub fn new() -> Self {
        Self { fonts: FontSystem::new(), cache: SwashCache::new(), loaded_fonts: HashSet::new(), images: HashMap::new(), layouts: HashMap::new(), bases: HashMap::new() }
    }

    fn buffer(&mut self, text: &str, style: &Style) -> Buffer {
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(style.size, style.size * 1.2));
        buffer.set_size(&mut self.fonts, None, None);
        let family = if let Some(name) = &style.font {
            Family::Name(name)
        } else if style.mono {
            Family::Monospace
        } else if style.serif {
            Family::Serif
        } else {
            Family::SansSerif
        };
        let attrs = Attrs::new().family(family).weight(if style.bold { Weight::BOLD } else { Weight::NORMAL });
        buffer.set_text(&mut self.fonts, text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.fonts, false);
        buffer
    }

    /// Makes the fonts a look ships available (each file is read once).
    pub fn ensure_fonts(&mut self, skin: &Skin) {
        let Some(dir) = &skin.dir else { return };
        for file in &skin.fonts {
            let path = dir.join(file);
            if self.loaded_fonts.insert(path.clone()) {
                self.fonts.db_mut().load_font_file(&path).ok();
                // Text laid out before may have used a stand-in font.
                self.layouts.clear();
            }
        }
    }

    /// Lays `text` out once; later calls reuse it.
    fn layout(&mut self, text: &str, style: &Style) -> LayoutKey {
        let key = layout_key(text, style);
        if !self.layouts.contains_key(&key) {
            if self.layouts.len() >= 512 {
                self.layouts.clear();
            }
            let buffer = self.buffer(text, style);
            self.layouts.insert(key.clone(), buffer);
        }
        key
    }

    /// A look's unchanging part at `scale`, if it was drawn already.
    pub(crate) fn base(&self, skin: &Skin, scale: f32) -> Option<&Pixmap> {
        self.bases.get(&(std::ptr::from_ref(skin) as usize, scale.to_bits()))
    }

    pub(crate) fn keep_base(&mut self, skin: &Skin, scale: f32, base: Pixmap) {
        if self.bases.len() >= 64 {
            self.bases.clear();
        }
        self.bases.insert((std::ptr::from_ref(skin) as usize, scale.to_bits()), base);
    }

    /// A PNG from a look's folder, decoded once.
    pub fn image(&mut self, path: &Path) -> Option<&Pixmap> {
        self.images.entry(path.to_path_buf()).or_insert_with(|| std::fs::read(path).ok().and_then(|bytes| Pixmap::decode_png(&bytes).ok())).as_ref()
    }

    /// Width and height `text` takes in `style`.
    pub fn measure(&mut self, text: &str, style: &Style) -> (f32, f32) {
        let key = self.layout(text, style);
        let buffer = &self.layouts[&key];
        let width = buffer.layout_runs().map(|run| run.line_w).fold(0.0, f32::max);
        (width, style.size * 1.2)
    }

    /// Draws `text` with its top-left at (`x`, `y`).
    pub fn draw(&mut self, pixmap: &mut Pixmap, text: &str, (x, y): (f32, f32), style: &Style) {
        let key = self.layout(text, style);
        let Text { fonts, cache, layouts, .. } = self;
        let buffer = &layouts[&key];
        let [r, g, b, a] = style.color;
        let (width, height) = (pixmap.width() as i32, pixmap.height() as i32);
        let (x0, y0) = (x.round() as i32, y.round() as i32);
        let data = pixmap.data_mut();
        buffer.draw(fonts, cache, Color::rgba(r, g, b, a), |gx, gy, _, _, color| {
            let (px, py) = (x0 + gx, y0 + gy);
            if px < 0 || py < 0 || px >= width || py >= height {
                return;
            }
            let alpha = u32::from(color.a());
            if alpha == 0 {
                return;
            }
            // Source-over onto a premultiplied pixel.
            let at = ((py * width + px) * 4) as usize;
            let inverse = 255 - alpha;
            for (channel, value) in [color.r(), color.g(), color.b()].into_iter().enumerate() {
                let premultiplied = u32::from(value) * alpha / 255;
                data[at + channel] = (premultiplied + u32::from(data[at + channel]) * inverse / 255).min(255) as u8;
            }
            data[at + 3] = (alpha + u32::from(data[at + 3]) * inverse / 255).min(255) as u8;
        });
    }
}

impl Default for Text {
    fn default() -> Self {
        Self::new()
    }
}
