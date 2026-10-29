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
}

#[derive(Debug, Clone)]
pub enum Message {
    Load,
    Loaded(Result<Data, String>),
    Install(String),
    Done(String, Result<(), String>),
    Remove(String),
    Removed(Result<(), String>),
}

enum Status {
    Idle,
    Loading,
    Failed(String),
    Ready { base: String, packs: Vec<Pack> },
}

pub struct State {
    status: Status,
    previews: HashMap<String, image::Handle>,
    installing: HashSet<String>,
    error: Option<String>,
}

impl State {
    pub fn new() -> Self {
        Self { status: Status::Idle, previews: HashMap::new(), installing: HashSet::new(), error: None }
    }

    /// Reads the catalogue the first time the tab is opened.
    pub fn opened(&mut self) -> Task<Message> {
        if matches!(self.status, Status::Idle) {
            return self.update(Message::Load).0;
        }
        Task::none()
    }

    /// Returns what to do next, and whether the installed looks changed.
    pub fn update(&mut self, message: Message) -> (Task<Message>, bool) {
        match message {
            Message::Load => {
                self.status = Status::Loading;
                self.error = None;
                (
                    blocking(
                        || {
                            let (index, base) = looks::fetch_index()?;
                            let previews = index.packs.iter().filter_map(|pack| Some((pack.id.clone(), on_wallpaper(&looks::fetch_preview(base, pack).ok()?)?))).collect();
                            Ok(Data { base: base.to_string(), packs: index.packs, previews })
                        },
                        Message::Loaded,
                    ),
                    false,
                )
            }
            Message::Loaded(Ok(data)) => {
                self.previews = data.previews.into_iter().collect();
                self.status = Status::Ready { base: data.base, packs: data.packs };
                (Task::none(), false)
            }
            Message::Loaded(Err(err)) => {
                self.status = Status::Failed(err);
                (Task::none(), false)
            }
            Message::Install(id) => {
                let Status::Ready { base, packs } = &self.status else { return (Task::none(), false) };
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
            Message::Removed(result) => {
                self.error = result.err().map(|err| format!("Couldn't remove it: {err}"));
                paperloom_skin::refresh();
                (Task::none(), true)
            }
        }
    }

    pub fn view<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        match &self.status {
            Status::Idle | Status::Loading => text("Reading the online library…").size(13).color(t.muted_foreground).into(),
            Status::Failed(err) => column![
                text("Couldn't reach the online library.").size(14).color(t.foreground),
                text(err.as_str()).size(12).color(t.muted_foreground),
                button(ui::label("Try again")).style(ui::secondary).on_press(Message::Load),
            ]
            .spacing(8)
            .into(),
            Status::Ready { packs, .. } => {
                let mut cards: Vec<Element<'a, Message>> = packs.iter().map(|pack| self.card(pack, t)).collect();
                if cards.is_empty() {
                    cards.push(text("Nothing here yet.").size(13).color(t.muted_foreground).into());
                }
                let mut page = column![].spacing(12);
                if let Some(err) = &self.error {
                    page = page.push(text(err.as_str()).size(13).color(t.destructive));
                }
                page.push(row(cards).spacing(16).wrap().vertical_spacing(16)).into()
            }
        }
    }

    fn card<'a>(&'a self, pack: &'a Pack, t: theme::Tokens) -> Element<'a, Message> {
        let installed = paperloom_skin::get(&pack.id).is_some_and(|skin| skin.dir.is_some());
        let action: Element<'a, Message> = if self.installing.contains(&pack.id) {
            text("Downloading…").size(13).color(t.primary).into()
        } else if installed {
            button(ui::label("Remove")).style(ui::ghost).on_press(Message::Remove(pack.folder.clone())).into()
        } else {
            button(ui::label(format!("Install · {}", size(pack.download_size()))).color(ui::on_accent())).style(ui::primary).on_press(Message::Install(pack.id.clone())).into()
        };
        let kinds = pack.kinds.iter().map(|kind| paperloom_common::widgets::entry(kind).map_or(kind.as_str(), |entry| entry.name)).collect::<Vec<_>>().join(" · ");
        let detail = if installed { format!("Installed · {kinds}") } else { format!("by {} · {kinds}", pack.author) };
        card(self.previews.get(&pack.id), pack.name.clone(), detail, vec![action], None, installed, t)
    }
}

fn size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    }
}
