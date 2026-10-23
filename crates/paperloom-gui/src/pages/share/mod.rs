//! Sharing wallpapers: sending a list of them to a friend, and importing one.
//!
//! A share is a small list of where each wallpaper comes from (see
//! `paperloom_fetch::share`). The *sender* dialog hands out the text (copy or
//! file); the *import* dialog reads that text, previews what's inside, and
//! puts the chosen wallpapers on the normal download queue, optionally into a
//! collection. The library page owns this state and the data it works on.

mod pending;
mod view;

use std::collections::HashMap;
use std::path::PathBuf;

use iced::Task;
use paperloom_common::{Library, Settings};
use paperloom_fetch::share::{allowed_hosts, Share, ShareError, CODE_COMFORT};
use paperloom_fetch::Queue;

use super::online::blocking;

/// Rendition to download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    Hd,
    Best,
}

impl std::fmt::Display for Quality {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Quality::Hd => "HD",
            Quality::Best => "4K when there is one",
        })
    }
}

pub const QUALITIES: [Quality; 2] = [Quality::Hd, Quality::Best];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItemState {
    New,
    InLibrary,
    Queued,
}

struct Sender {
    title: String,
    share: Share,
    code: String,
    notice: Option<String>,
    /// The sender's own posters, by wallpaper key, for the preview.
    posters: HashMap<String, PathBuf>,
}

struct Import {
    text: String,
    error: Option<String>,
    share: Option<Share>,
    states: Vec<ItemState>,
    picked: Vec<bool>,
    quality: Quality,
    collection: String,
}

#[derive(Default)]
pub struct State {
    sender: Option<Sender>,
    import: Option<Import>,
    thumbs: HashMap<String, PathBuf>,
}

#[derive(Debug, Clone)]
pub enum Message {
    // --- Sending
    CloseSender,
    CopyCode,
    SaveFile,
    // --- Importing
    OpenImport,
    CloseImport,
    Text(String),
    Paste,
    Pasted(Option<String>),
    ChooseFile,
    Toggle(usize, bool),
    PickAll(bool),
    Quality(Quality),
    Collection(String),
    Start,
    Thumb(String, Result<PathBuf, String>),
}

/// What the dialogs work on, borrowed from the library page and the app.
pub struct Ctx<'a> {
    pub settings: &'a Settings,
    pub library: &'a mut Library,
    pub queue: &'a Queue,
    pub status: &'a mut Option<String>,
}

impl State {
    /// Opens the sender dialog for `share`, called `title`.
    pub fn send(&mut self, title: String, share: Share, posters: HashMap<String, PathBuf>) {
        let code = share.encode();
        self.sender = Some(Sender { title, share, code, notice: None, posters });
    }

    /// Opens the import dialog, already holding `text` (a pasted code or a
    /// file's contents) when there is some.
    pub fn open_import(&mut self, text: &str, ctx: &Ctx) -> Task<Message> {
        self.import = Some(Import { text: String::new(), error: None, share: None, states: Vec::new(), picked: Vec::new(), quality: Quality::Hd, collection: String::new() });
        if text.trim().is_empty() {
            Task::none()
        } else {
            self.read(text, ctx)
        }
    }

    /// Parses `text` into the import dialog's preview.
    fn read(&mut self, text: &str, ctx: &Ctx) -> Task<Message> {
        let Some(import) = &mut self.import else { return Task::none() };
        import.text = text.chars().take(200).collect();
        import.error = None;
        import.share = None;
        match Share::decode(text, &allowed_hosts(&ctx.settings.sources)) {
            Err(err) => {
                import.error = Some(err.to_string());
                Task::none()
            }
            Ok(share) => {
                let queued: Vec<String> = ctx.queue.snapshot().into_iter().map(|job| job.id).collect();
                import.states = share
                    .items
                    .iter()
                    .map(|item| {
                        let key = item.key();
                        if ctx.library.contains_source(&key) {
                            ItemState::InLibrary
                        } else if queued.contains(&key) {
                            ItemState::Queued
                        } else {
                            ItemState::New
                        }
                    })
                    .collect();
                import.picked = import.states.iter().map(|state| *state == ItemState::New).collect();
                import.collection = share.name.clone();
                let wanted: Vec<(String, String)> =
                    share.items.iter().filter(|item| !item.th.is_empty()).take(80).map(|item| (item.key(), item.th.clone())).filter(|(key, _)| !self.thumbs.contains_key(key)).collect();
                let mut tasks: Vec<Task<Message>> = Vec::new();
                for (key, url) in wanted {
                    if let Some(path) = paperloom_fetch::cached_thumbnail(&url) {
                        self.thumbs.insert(key, path);
                        continue;
                    }
                    tasks.push(Task::perform(blocking(move || paperloom_fetch::fetch_thumbnail(&url).map_err(|err| err.to_string())), move |result| Message::Thumb(key.clone(), result)));
                }
                import.share = Some(share);
                Task::batch(tasks)
            }
        }
    }

    pub fn update(&mut self, message: Message, ctx: &mut Ctx) -> Task<Message> {
        match message {
            Message::CloseSender => self.sender = None,
            Message::CopyCode => {
                if let Some(sender) = &mut self.sender {
                    sender.notice = Some("Copied. Paste it in a chat for your friend.".into());
                    return iced::clipboard::write(sender.code.clone());
                }
            }
            Message::SaveFile => {
                if let Some(sender) = &mut self.sender {
                    let name = if sender.share.name.is_empty() { "wallpapers".to_string() } else { sender.share.name.clone() };
                    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
                    let picked = rfd::FileDialog::new().set_title("Save share file").set_file_name(format!("{safe}.paperloomshare")).add_filter("Paperloom share", &["paperloomshare"]).save_file();
                    if let Some(path) = picked {
                        sender.notice = Some(match std::fs::write(&path, &sender.code) {
                            Ok(()) => format!("Saved {}", path.display()),
                            Err(err) => format!("Couldn't save the file: {err}"),
                        });
                    }
                }
            }
            Message::OpenImport => return self.open_import("", ctx),
            Message::CloseImport => self.import = None,
            Message::Text(text) => return self.read(&text, ctx),
            Message::Paste => return iced::clipboard::read().map(Message::Pasted),
            Message::Pasted(text) => {
                if let Some(text) = text {
                    return self.read(&text, ctx);
                }
            }
            Message::ChooseFile => {
                let picked = rfd::FileDialog::new().set_title("Open a share file").add_filter("Paperloom share", &["paperloomshare", "txt"]).pick_file();
                if let Some(path) = picked {
                    match std::fs::read_to_string(&path) {
                        Ok(text) => return self.read(&text, ctx),
                        Err(_) => {
                            if let Some(import) = &mut self.import {
                                import.error = Some(ShareError::Damaged.to_string());
                            }
                        }
                    }
                }
            }
            Message::Toggle(index, on) => {
                if let Some(slot) = self.import.as_mut().and_then(|import| import.picked.get_mut(index)) {
                    *slot = on;
                }
            }
            Message::PickAll(on) => {
                if let Some(import) = &mut self.import {
                    let states = import.states.clone();
                    for (slot, state) in import.picked.iter_mut().zip(states) {
                        *slot = on && state == ItemState::New;
                    }
                }
            }
            Message::Quality(quality) => {
                if let Some(import) = &mut self.import {
                    import.quality = quality;
                }
            }
            Message::Collection(name) => {
                if let Some(import) = &mut self.import {
                    import.collection = name;
                }
            }
            Message::Start => self.start(ctx),
            Message::Thumb(key, result) => {
                if let Ok(path) = result {
                    self.thumbs.insert(key, path);
                }
            }
        }
        Task::none()
    }

    /// Queues the chosen wallpapers and files them under the chosen collection.
    fn start(&mut self, ctx: &mut Ctx) {
        let Some(import) = self.import.take() else { return };
        let Some(share) = &import.share else { return };
        let collection = import.collection.trim().to_string();
        let (mut queued, mut have, mut waiting) = (0, 0, Vec::new());
        for (index, item) in share.items.iter().enumerate() {
            let key = item.key();
            let wanted = import.picked.get(index).copied().unwrap_or(false);
            // Already in the library: nothing to download, but it still joins the collection.
            if import.states.get(index) == Some(&ItemState::InLibrary) {
                if !collection.is_empty() {
                    let _ = ctx.library.add_source_to_collection(&key, &collection);
                }
                have += 1;
                continue;
            }
            if !wanted {
                continue;
            }
            let label = if import.quality == Quality::Best && item.has_4k() {
                "4K".to_string()
            } else if item.v.iter().any(|(label, _)| label == "HD") {
                "HD".to_string()
            } else {
                item.v.first().map(|(label, _)| label.clone()).unwrap_or_default()
            };
            ctx.queue.add(&item.online_item(), &label);
            queued += 1;
            waiting.push(key);
        }
        if !collection.is_empty() {
            pending::add(&waiting, &collection);
        }
        *ctx.status = Some(match (queued, have) {
            (0, 0) => "Nothing was picked.".to_string(),
            (0, _) => format!("You already have {have} of these."),
            (_, 0) => format!("Downloading {queued} wallpaper{}. Follow them on the Downloads page.", plural(queued)),
            _ => format!("Downloading {queued} wallpaper{}; you already had {have}. Follow them on the Downloads page.", plural(queued)),
        });
    }

    /// A wallpaper that has just been imported from a share download joins
    /// the collection it was promised to.
    pub fn finished(library: &mut Library, key: &str) {
        if let Some(name) = pending::take(key) {
            let _ = library.add_source_to_collection(key, &name);
        }
    }

    pub fn code_is_long(share: &Share) -> bool {
        share.items.len() > CODE_COMFORT
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

/// A download was cancelled, so any collection it was promised goes too.
pub fn forget_promise(key: &str) {
    pending::forget(key);
}
