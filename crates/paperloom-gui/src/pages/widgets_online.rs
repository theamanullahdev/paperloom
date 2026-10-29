//! Browse → Online: the catalogue of downloadable looks. It is read from the
//! repository when the tab opens (so there is nothing to wait for on a normal
//! launch), previews come with it, and installing a look downloads its few
//! files and checks each against the catalogue's checksum. Looks that are
//! installed show up in the Installed tab at once.

use std::collections::{HashMap, HashSet};

use iced::widget::{button, column, image, row, text};
use iced::{Element, Task};

use paperloom_fetch::looks::{self, Pack};

use super::widgets::card;
use super::widgets_looks::{blocking, on_wallpaper};
use crate::theme;
use crate::ui;

#[derive(Debug, Clone)]
pub struct Data {
    base: String,
    packs: Vec<Pack>,
    /// Each look's preview, already drawn on the user's wallpaper.
    previews: Vec<(String, image::Handle)>,
    /// The network was down: this is the catalogue as last read.
    offline: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Load,
    Loaded(Result<Data, String>),
    Install(String),
    Done(String, Result<(), String>),
    Remove(String),
    Removed(Result<(), String>),
    /// Use this installed look on a widget (handled by the widgets page).
    Use(String),
    /// Put a new widget of this kind with this look on the desktop (handled by the widgets page).
    Add(String, String),
}

enum Status {
    Idle,
    Loading,
    Failed(String),
    Ready { base: String, packs: Vec<Pack>, offline: bool },
}

/// For a look's kinds and id: the widget "Use" would change, and whether it already uses it.
pub type Target<'t> = dyn Fn(&[String], &str) -> Option<(String, bool)> + 't;

pub struct State {
    status: Status,
    previews: HashMap<String, image::Handle>,
    installing: HashSet<String>,
    error: Option<String>,
    /// When the catalogue was last asked for (a failed read is tried again).
    asked: std::time::Instant,
}

impl State {
    pub fn new() -> Self {
        Self { status: Status::Idle, previews: HashMap::new(), installing: HashSet::new(), error: None, asked: std::time::Instant::now() }
    }

    /// Reads the catalogue the first time the tab is opened.
    pub fn opened(&mut self) -> Task<Message> {
        if matches!(self.status, Status::Idle) {
            return self.update(Message::Load).0;
        }
        Task::none()
    }

    /// Tries again on its own every half minute while the network is down.
    pub fn retry(&mut self) -> Task<Message> {
        let down = matches!(self.status, Status::Failed(_) | Status::Ready { offline: true, .. });
        if down && self.asked.elapsed() > std::time::Duration::from_secs(30) {
            return self.update(Message::Load).0;
        }
        Task::none()
    }

    /// Returns what to do next, and whether the installed looks changed.
    pub fn update(&mut self, message: Message) -> (Task<Message>, bool) {
        match message {
            Message::Load => {
                self.asked = std::time::Instant::now();
                // A retry keeps what is on screen until the answer comes.
                if !matches!(self.status, Status::Ready { .. }) {
                    self.status = Status::Loading;
                }
                self.error = None;
                (
                    blocking(
                        || {
                            let cache = paperloom_common::online_cache_dir().join("looks");
                            let shown = |previews: Vec<(String, Vec<u8>)>| previews.into_iter().filter_map(|(id, bytes)| Some((id, on_wallpaper(&bytes)?))).collect();
                            match looks::fetch_index() {
                                Ok((index, base)) => {
                                    let raw: Vec<(String, Vec<u8>)> = index.packs.iter().filter_map(|pack| Some((pack.id.clone(), looks::fetch_preview(base, pack).ok()?))).collect();
                                    looks::save_cache(&cache, base, &index, &raw);
                                    Ok(Data { base: base.to_string(), packs: index.packs, previews: shown(raw), offline: false })
                                }
                                // Offline: the library as it was last time.
                                Err(err) => match looks::load_cache(&cache) {
                                    Some((base, index, raw)) => Ok(Data { base, packs: index.packs, previews: shown(raw), offline: true }),
                                    None => Err(err),
                                },
                            }
                        },
                        Message::Loaded,
                    ),
                    false,
                )
            }
            Message::Loaded(Ok(data)) => {
                self.previews = data.previews.into_iter().collect();
                self.status = Status::Ready { base: data.base, packs: data.packs, offline: data.offline };
                (Task::none(), false)
            }
            Message::Loaded(Err(err)) => {
                // Still showing a library: keep it.
                if !matches!(self.status, Status::Ready { .. }) {
                    self.status = Status::Failed(friendly(&err));
                }
                (Task::none(), false)
            }
            Message::Install(id) => {
                let Status::Ready { base, packs, .. } = &self.status else { return (Task::none(), false) };
                let Some(pack) = packs.iter().find(|pack| pack.id == id).cloned() else { return (Task::none(), false) };
                let base = base.clone();
                self.installing.insert(id.clone());
                self.error = None;
                (blocking(move || looks::install(&base, &pack, &paperloom_common::skins_dir()), move |result| Message::Done(id.clone(), result)), false)
            }
            Message::Done(id, result) => {
                self.installing.remove(&id);
                match result {
                    Ok(()) => {
                        paperloom_skin::refresh();
                        (Task::none(), true)
                    }
                    Err(err) => {
                        self.error = Some(format!("Couldn't install it: {err}"));
                        (Task::none(), false)
                    }
                }
            }
            Message::Remove(folder) => (blocking(move || looks::remove(&folder, &paperloom_common::skins_dir()), Message::Removed), false),
            Message::Use(_) | Message::Add(..) => (Task::none(), false),
            Message::Removed(result) => {
                self.error = result.err().map(|err| format!("Couldn't remove it: {err}"));
                paperloom_skin::refresh();
                (Task::none(), true)
            }
        }
    }

    /// `filter`: a widget kind, or empty for all. `target`: for a look's kinds and
    /// id, the widget "Use" would change and whether it already uses it.
    pub fn view<'a>(&'a self, filter: &str, target: &Target, t: theme::Tokens) -> Element<'a, Message> {
        match &self.status {
            Status::Idle | Status::Loading => text("Reading the online library…").size(13).color(t.muted_foreground).into(),
            Status::Failed(err) => column![
                text("Couldn't reach the online library.").size(14).color(t.foreground),
                text(err.as_str()).size(12).color(t.muted_foreground),
                text("It tries again on its own every half minute.").size(12).color(t.muted_foreground),
                button(ui::label("Try again now")).style(ui::secondary).on_press(Message::Load),
            ]
            .spacing(8)
            .into(),
            Status::Ready { packs, offline, .. } => {
                let mut cards: Vec<Element<'a, Message>> = packs.iter().filter(|pack| filter.is_empty() || pack.kinds.iter().any(|kind| kind == filter)).map(|pack| self.card(pack, filter, target, t)).collect();
                if cards.is_empty() {
                    cards.push(text("Nothing here yet.").size(13).color(t.muted_foreground).into());
                }
                let mut page = column![].spacing(12);
                if *offline {
                    page = page.push(
                        row![
                            text("This computer is offline, so this is the library as it was last time. Installing needs a connection.").size(13).color(t.muted_foreground),
                            button(ui::label("Try again")).style(ui::ghost).on_press(Message::Load),
                        ]
                        .spacing(10)
                        .align_y(iced::Alignment::Center),
                    );
                }
                if let Some(err) = &self.error {
                    page = page.push(text(err.as_str()).size(13).color(t.destructive));
                }
                page.push(row(cards).spacing(16).wrap().vertical_spacing(16)).into()
            }
        }
    }

    fn card<'a>(&'a self, pack: &'a Pack, filter: &str, target: &Target, t: theme::Tokens) -> Element<'a, Message> {
        let installed = paperloom_skin::get(&pack.id).is_some_and(|skin| skin.dir.is_some());
        let kinds = pack.kinds.iter().map(|kind| paperloom_common::widgets::entry(kind).map_or(kind.as_str(), |entry| entry.name)).collect::<Vec<_>>().join(" · ");
        let mut detail = format!("by {} · {kinds}", pack.author);
        let mut actions: Vec<Element<'a, Message>> = Vec::new();
        if self.installing.contains(&pack.id) {
            actions.push(text("Downloading…").size(13).color(t.primary).into());
        } else if installed {
            // Installed: the next step is right here.
            match target(&pack.kinds, &pack.id) {
                Some((title, true)) => detail = format!("In use on your {title}"),
                Some((title, false)) => actions.push(button(ui::label(format!("Use on {title}")).color(ui::on_accent())).style(ui::primary).on_press(Message::Use(pack.id.clone())).into()),
                None => {}
            }
            let kind = if pack.kinds.iter().any(|kind| kind == filter) { filter.to_string() } else { pack.kinds.first().cloned().unwrap_or_default() };
            actions.push(button(ui::label("Add")).style(ui::secondary).on_press(Message::Add(kind, pack.id.clone())).into());
            actions.push(button(ui::label("Remove")).style(ui::ghost).on_press(Message::Remove(pack.folder.clone())).into());
        } else {
            actions.push(button(ui::label(format!("Install · {}", size(pack.download_size()))).color(ui::on_accent())).style(ui::primary).on_press(Message::Install(pack.id.clone())).into());
        }
        card(self.previews.get(&pack.id), pack.name.clone(), detail, actions, None, installed, t)
    }
}

/// The network's own words, in plain ones.
fn friendly(err: &str) -> String {
    let lower = err.to_ascii_lowercase();
    if lower.contains("lookup") || lower.contains("resolve") || lower.contains("dns") || lower.contains("network is unreachable") {
        "This computer looks offline (the library's address couldn't be found).".into()
    } else if lower.contains("timed out") || lower.contains("timeout") {
        "The library took too long to answer.".into()
    } else {
        err.to_string()
    }
}

fn size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    }
}
