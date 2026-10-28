//! Looks in the app: which look a placed widget uses, its size, and the
//! gallery's previews. Previews are drawn by the same renderer as the desktop
//! (`paperloom-skin`), once, on a background thread (loading the system fonts
//! takes a moment), then kept as images.

use std::collections::HashMap;

use iced::widget::image;
use iced::Task;
use paperloom_common::widgets::Placed;
use paperloom_skin::Skin;
use tiny_skia::{Color, Pixmap, PixmapPaint, Transform};

/// The look a placed widget uses: the one it picked, else its kind's default.
pub fn skin_of(placed: &Placed) -> Option<&'static Skin> {
    paperloom_skin::get(&placed.skin).filter(|skin| skin.kinds.contains(&placed.kind)).or_else(|| paperloom_skin::default_for(&placed.kind))
}

/// Natural size in pixels at scale 1.0.
pub fn size_of(placed: &Placed) -> (u32, u32) {
    skin_of(placed).map_or((230, 104), |skin| (skin.size[0] as u32, skin.size[1] as u32))
}

/// A look's preview, ready to show.
#[derive(Clone)]
pub struct Preview {
    pub handle: image::Handle,
    pub size: (u32, u32),
}

pub type Previews = HashMap<String, Preview>;

/// A drawn preview before it is turned into an image: id, size, straight-alpha RGBA.
pub type Drawn = Vec<(String, u32, u32, Vec<u8>)>;

/// Draws every look on a backdrop (so translucent cards read as they will on a
/// wallpaper), on a thread of its own.
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
    drawn.into_iter().map(|(id, w, h, rgba)| (id, Preview { handle: image::Handle::from_rgba(w, h, rgba), size: (w, h) })).collect()
}

fn draw_all() -> Drawn {
    let mut text = paperloom_skin::Text::new();
    let mut drawn = Vec::new();
    for skin in paperloom_skin::all() {
        let kind = skin.kinds.first().map_or("", String::as_str);
        let widget = paperloom_skin::render(skin, &paperloom_skin::preview(kind), 1.0, &mut text);
        let pad = 18u32;
        let (w, h) = (widget.width() + pad * 2, widget.height() + pad * 2);
        let Some(mut backdrop) = Pixmap::new(w, h) else { continue };
        backdrop.fill(Color::from_rgba8(0x3A, 0x4A, 0x6A, 0xFF));
        backdrop.draw_pixmap(pad as i32, pad as i32, widget.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        // The backdrop is opaque, so premultiplied and straight alpha agree.
        drawn.push((skin.id.clone(), w, h, backdrop.take()));
    }
    drawn
}
