//! Looks in the app: which look a placed widget uses, its size, and every
//! preview. Previews are drawn by the same renderer as the desktop
//! (`paperloom-skin`) on top of the user's own wallpaper — the live
//! wallpaper's poster when one is playing, else the system background — so a
//! card shows what the widget will look like on their screen, at the same
//! proportions. Drawn once on a background thread (loading the system fonts
//! and decoding the wallpaper take a moment), then kept as images.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use iced::widget::image;
use iced::Task;
use paperloom_common::widgets::Placed;
use paperloom_skin::Skin;
use tiny_skia::{Color, FilterQuality, IntSize, Pixmap, PixmapPaint, Transform};

/// Every card has this size, whatever the look's own size.
pub const CARD: (u32, u32) = (300, 170);

/// The look a placed widget uses: the one it picked, else its kind's default.
pub fn skin_of(placed: &Placed) -> Option<&'static Skin> {
    paperloom_skin::get(&placed.skin).filter(|skin| skin.kinds.contains(&placed.kind)).or_else(|| paperloom_skin::default_for(&placed.kind))
}

/// Natural size in pixels at scale 1.0.
pub fn size_of(placed: &Placed) -> (u32, u32) {
    skin_of(placed).map_or((230, 104), |skin| (skin.size[0] as u32, skin.size[1] as u32))
}

/// Every preview, keyed by look and kind (a meter look shows the processor
/// on one card and the memory on another).
#[derive(Default)]
pub struct Previews {
    /// On a piece of the wallpaper, `CARD` sized.
    cards: HashMap<(String, String), image::Handle>,
    /// The widget alone, at its natural size (Arrange draws these).
    bare: HashMap<(String, String), image::Handle>,
    /// The wallpaper itself, for the screen pictures.
    pub wallpaper: Option<image::Handle>,
    /// Each stack preset on a piece of the wallpaper.
    stacks: HashMap<String, image::Handle>,
}

impl Previews {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }


    pub fn stack(&self, preset: &str) -> Option<&image::Handle> {
        self.stacks.get(preset)
    }

    pub fn card(&self, skin: &str, kind: &str) -> Option<&image::Handle> {
        self.cards.get(&(skin.to_string(), kind.to_string()))
    }

    pub fn bare(&self, skin: &str, kind: &str) -> Option<&image::Handle> {
        self.bare.get(&(skin.to_string(), kind.to_string()))
    }

    /// The card for a placed widget.
    pub fn of(&self, placed: &Placed) -> Option<&image::Handle> {
        skin_of(placed).and_then(|skin| self.card(&skin.id, &placed.kind))
    }
}

/// Straight-alpha RGBA and its size.
type Rgba = (u32, u32, Vec<u8>);

/// Drawn previews before they are turned into images.
#[derive(Debug, Clone, Default)]
pub struct Drawn {
    wallpaper: Option<Rgba>,
    looks: Vec<(String, String, Rgba, Rgba)>,
    stacks: Vec<(String, Rgba)>,
}

/// Draws every look on the wallpaper, on a thread of its own.
pub fn load<M: Send + 'static>(done: impl Fn(Drawn) -> M + Send + 'static) -> Task<M> {
    let (sender, receiver) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = sender.send(draw_all());
    });
    Task::perform(async move { receiver.await.unwrap_or_default() }, done)
}

/// Runs `work` on a thread of its own and hands the result to `done` (the
/// network and the disk never run on the interface's thread).
pub fn blocking<T: Send + 'static, M: Send + 'static>(work: impl FnOnce() -> T + Send + 'static, done: impl Fn(T) -> M + Send + 'static) -> Task<M> {
    let (sender, receiver) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = sender.send(work());
    });
    Task::future(receiver).then(move |outcome| match outcome {
        Ok(value) => Task::done(done(value)),
        Err(_) => Task::none(),
    })
}

pub fn into_previews(drawn: Drawn) -> Previews {
    let mut previews = Previews::default();
    previews.merge(drawn);
    previews
}

/// Redraws only the clocks and dates (they show the real time), on the
/// wallpaper already decoded.
pub fn load_times<M: Send + 'static>(done: impl Fn(Drawn) -> M + Send + 'static) -> Task<M> {
    blocking(|| draw(|kind| kind == "clock" || kind == "date", false), done)
}

impl Previews {
    /// Takes in newly drawn previews (all of them, or only some).
    pub fn merge(&mut self, drawn: Drawn) {
        let handle = |(w, h, rgba): Rgba| image::Handle::from_rgba(w, h, rgba);
        if let Some(wallpaper) = drawn.wallpaper {
            self.wallpaper = Some(handle(wallpaper));
        }
        for (id, card) in drawn.stacks {
            self.stacks.insert(id, handle(card));
        }
        for (skin, kind, card, bare) in drawn.looks {
            self.cards.insert((skin.clone(), kind.clone()), handle(card));
            self.bare.insert((skin, kind), handle(bare));
        }
    }
}

/// The wallpaper the last previews were drawn on (the online library's
/// previews are drawn on it too).
static WALLPAPER: Mutex<Option<Pixmap>> = Mutex::new(None);

/// A ready-drawn widget picture (the online library's previews) put on a
/// card-sized piece of the wallpaper, like every other preview.
pub fn on_wallpaper(png: &[u8]) -> Option<image::Handle> {
    let widget = Pixmap::decode_png(png).ok()?;
    let screen = paperloom_common::primary_size().unwrap_or((1920, 1080));
    let mut cached = WALLPAPER.lock().ok()?;
    if cached.is_none() {
        *cached = wallpaper();
    }
    let fit = (0.62 * CARD.0 as f32 / widget.width() as f32).min(0.62 * CARD.1 as f32 / widget.height() as f32).min(1.0);
    let mut card = backdrop(cached.as_ref(), screen, fit)?;
    let (w, h) = (widget.width() as f32 * fit, widget.height() as f32 * fit);
    let at = Transform::from_row(fit, 0.0, 0.0, fit, (CARD.0 as f32 - w) / 2.0, (CARD.1 as f32 - h) / 2.0);
    card.draw_pixmap(0, 0, widget.as_ref(), &PixmapPaint { quality: FilterQuality::Bilinear, ..PixmapPaint::default() }, at, None);
    Some(image::Handle::from_rgba(CARD.0, CARD.1, card.take()))
}

fn draw_all() -> Drawn {
    draw(|_| true, true)
}

/// Draws the looks of the kinds `wanted` picks; `fresh` reads the wallpaper again.
fn draw(wanted: impl Fn(&str) -> bool, fresh: bool) -> Drawn {
    let screen = paperloom_common::primary_size().unwrap_or((1920, 1080));
    let cached = WALLPAPER.lock().ok().and_then(|cached| cached.clone());
    let wallpaper = if fresh || cached.is_none() { wallpaper() } else { cached };
    if let Ok(mut cached) = WALLPAPER.lock() {
        cached.clone_from(&wallpaper);
    }
    let mut text = paperloom_skin::Text::new();
    let mut looks = Vec::new();
    for skin in paperloom_skin::all() {
        for kind in skin.kinds.iter().filter(|kind| wanted(kind)) {
            let vars = paperloom_skin::preview(kind);
            let bare = paperloom_skin::render(skin, &vars, 1.0, &mut text);
            // Shrink it (and the wallpaper with it) until it fits the card with room around it.
            let fit = (0.62 * CARD.0 as f32 / bare.width() as f32).min(0.62 * CARD.1 as f32 / bare.height() as f32).min(1.0);
            let widget = if fit < 1.0 { paperloom_skin::render(skin, &vars, fit, &mut text) } else { bare.clone() };
            let Some(mut card) = backdrop(wallpaper.as_ref(), screen, fit) else { continue };
            let x = (CARD.0 as i32 - widget.width() as i32) / 2;
            let y = (CARD.1 as i32 - widget.height() as i32) / 2;
            card.draw_pixmap(x, y, widget.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
            looks.push((skin.id.clone(), kind.clone(), (CARD.0, CARD.1, card.take()), straight(&bare)));
        }
    }
    // Stack presets: every widget of the set, drawn where it sits in it.
    let mut stacks = Vec::new();
    if wanted("clock") {
        for preset in paperloom_common::stacks::presets() {
            let fit = (0.9 * CARD.0 as f32 / preset.size[0]).min(0.85 * CARD.1 as f32 / preset.size[1]).min(1.0);
            let Some(mut card) = backdrop(wallpaper.as_ref(), screen, fit) else { continue };
            let origin = ((CARD.0 as f32 - preset.size[0] * fit) / 2.0, (CARD.1 as f32 - preset.size[1] * fit) / 2.0);
            for member in &preset.widget {
                let look = paperloom_skin::get(&member.skin).filter(|look| look.kinds.contains(&member.kind)).or_else(|| paperloom_skin::default_for(&member.kind));
                let Some(look) = look else { continue };
                let picture = paperloom_skin::render(look, &paperloom_skin::preview(&member.kind), fit, &mut text);
                card.draw_pixmap((origin.0 + member.dx * fit) as i32, (origin.1 + member.dy * fit) as i32, picture.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
            }
            stacks.push((preset.id.clone(), (CARD.0, CARD.1, card.take())));
        }
    }
    let wallpaper = wallpaper.filter(|_| fresh).map(|pixmap| (pixmap.width(), pixmap.height(), pixmap.take()));
    Drawn { wallpaper, looks, stacks }
}

/// A `CARD`-sized piece of the middle of the screen, `zoom` times its real size.
fn backdrop(wallpaper: Option<&Pixmap>, screen: (u32, u32), zoom: f32) -> Option<Pixmap> {
    let mut card = Pixmap::new(CARD.0, CARD.1)?;
    let Some(wallpaper) = wallpaper else {
        card.fill(Color::from_rgba8(0x3A, 0x4A, 0x6A, 0xFF));
        return Some(card);
    };
    // The wallpaper fills the screen (cover), then the card looks at its middle.
    let cover = (screen.0 as f32 / wallpaper.width() as f32).max(screen.1 as f32 / wallpaper.height() as f32);
    let scale = cover * zoom;
    let dx = CARD.0 as f32 / 2.0 - wallpaper.width() as f32 * scale / 2.0;
    let dy = CARD.1 as f32 / 2.0 - wallpaper.height() as f32 * scale / 2.0;
    let paint = PixmapPaint { quality: FilterQuality::Bilinear, ..PixmapPaint::default() };
    card.draw_pixmap(0, 0, wallpaper.as_ref(), &paint, Transform::from_row(scale, 0.0, 0.0, scale, dx, dy), None);
    Some(card)
}

/// Premultiplied (tiny-skia) to straight alpha (iced).
fn straight(pixmap: &Pixmap) -> Rgba {
    let mut rgba = Vec::with_capacity(pixmap.data().len());
    for pixel in pixmap.pixels() {
        let color = pixel.demultiply();
        rgba.extend_from_slice(&[color.red(), color.green(), color.blue(), color.alpha()]);
    }
    (pixmap.width(), pixmap.height(), rgba)
}

/// The wallpaper the user sees right now, at most 1280 pixels wide, opaque.
fn wallpaper() -> Option<Pixmap> {
    candidates().into_iter().find_map(|path| {
        let decoded = ::image::ImageReader::open(&path).ok()?.with_guessed_format().ok()?.decode().ok()?;
        let rgba = decoded.thumbnail(1280, 1280).to_rgba8();
        let (w, h) = rgba.dimensions();
        // Opaque: straight and premultiplied alpha agree.
        Pixmap::from_vec(rgba.into_raw(), IntSize::from_wh(w, h)?)
    })
}

/// Where the current wallpaper's picture might be, best first.
fn candidates() -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Some(current) = paperloom_common::EngineState::load().current_wallpaper {
        let library = paperloom_common::Library::load_manifest();
        if let Some(entry) = library.entries.iter().find(|entry| entry.path == current) {
            found.extend(entry.thumbnail.clone());
            found.extend(entry.preview_frames.first().cloned());
        }
        // A still picture used as the wallpaper.
        found.push(current);
    }
    found.extend(system_background());
    found
}

#[cfg(windows)]
fn system_background() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|appdata| PathBuf::from(appdata).join("Microsoft\\Windows\\Themes\\TranscodedWallpaper"))
}

#[cfg(target_os = "macos")]
fn system_background() -> Option<PathBuf> {
    let output = std::process::Command::new("osascript").args(["-e", "tell application \"System Events\" to get picture of current desktop"]).output().ok()?;
    let path = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn system_background() -> Option<PathBuf> {
    for (schema, key) in [("org.cinnamon.desktop.background", "picture-uri"), ("org.gnome.desktop.background", "picture-uri-dark"), ("org.gnome.desktop.background", "picture-uri"), ("org.mate.background", "picture-filename")] {
        let Ok(output) = std::process::Command::new("gsettings").args(["get", schema, key]).output() else { continue };
        let value = String::from_utf8_lossy(&output.stdout).trim().trim_matches('\'').to_string();
        let path = value.strip_prefix("file://").unwrap_or(&value).replace("%20", " ");
        if !path.is_empty() && std::path::Path::new(&path).is_file() {
            return Some(PathBuf::from(path));
        }
    }
    None
}
