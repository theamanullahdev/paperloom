//! Text into a pixmap. One `Text` owns the font database and glyph cache for
//! the whole process; both are bounded by what the widgets actually show.

use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache, Weight};
use tiny_skia::Pixmap;

pub struct Text {
    fonts: FontSystem,
    cache: SwashCache,
}

impl Text {
    pub fn new() -> Self {
        Self { fonts: FontSystem::new(), cache: SwashCache::new() }
    }

    fn buffer(&mut self, text: &str, size: f32, bold: bool) -> Buffer {
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(size, size * 1.2));
        buffer.set_size(&mut self.fonts, None, None);
        let attrs = Attrs::new().family(Family::SansSerif).weight(if bold { Weight::BOLD } else { Weight::NORMAL });
        buffer.set_text(&mut self.fonts, text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.fonts, false);
        buffer
    }

    /// Width and height `text` takes at `size`.
    pub fn measure(&mut self, text: &str, size: f32, bold: bool) -> (f32, f32) {
        let buffer = self.buffer(text, size, bold);
        let width = buffer.layout_runs().map(|run| run.line_w).fold(0.0, f32::max);
        (width, size * 1.2)
    }

    /// Draws `text` with its top-left at (`x`, `y`).
    pub fn draw(&mut self, pixmap: &mut Pixmap, text: &str, (x, y): (f32, f32), size: f32, bold: bool, [r, g, b, a]: [u8; 4]) {
        let buffer = self.buffer(text, size, bold);
        let (width, height) = (pixmap.width() as i32, pixmap.height() as i32);
        let (x0, y0) = (x.round() as i32, y.round() as i32);
        let data = pixmap.data_mut();
        buffer.draw(&mut self.fonts, &mut self.cache, Color::rgba(r, g, b, a), |gx, gy, _, _, color| {
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
