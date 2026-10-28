//! Browse → Online: the catalogue of downloadable looks. It is read from the
//! repository when the tab opens (so there is nothing to wait for on a normal
//! launch), previews come with it, and installing a look downloads its few
//! files and checks each against the catalogue's checksum. Looks that are
//! installed show up in the Installed tab at once.

use std::collections::{HashMap, HashSet};

use iced::widget::{button, column, container, image, row, text, Space};
use iced::{Alignment, Background, Border, Element, Length, Task};

use paperloom_fetch::looks::{self, Pack};

use super::widgets_looks::blocking;
use crate::theme;
use crate::ui;

#[derive(Debug, Clone)]
pub struct Data {
    base: String,
    packs: Vec<Pack>,
    previews: Vec<(String, Vec<u8>)>,
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
                            let previews = index.packs.iter().filter_map(|pack| looks::fetch_preview(base, pack).ok().map(|bytes| (pack.id.clone(), bytes))).collect();
                            Ok(Data { base: base.to_string(), packs: index.packs, previews })
                        },
                        Message::Loaded,
                    ),
                    false,
                )
            }
            Message::Loaded(Ok(data)) => {
                self.previews = data.previews.into_iter().map(|(id, bytes)| (id, image::Handle::from_bytes(bytes))).collect();
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
                page.push(row(cards).spacing(14).wrap().vertical_spacing(14)).into()
            }
        }
    }

    fn card<'a>(&'a self, pack: &'a Pack, t: theme::Tokens) -> Element<'a, Message> {
        let installed = paperloom_skin::get(&pack.id).is_some_and(|skin| skin.dir.is_some());
        let busy = self.installing.contains(&pack.id);
        let preview: Element<'a, Message> = match self.previews.get(&pack.id) {
            Some(handle) => image(handle.clone()).width(Length::Fixed(300.0)).into(),
            None => container(Space::new()).width(Length::Fixed(300.0)).height(Length::Fixed(140.0)).into(),
        };
        let action: Element<'a, Message> = if busy {
            text("Downloading…").size(13).color(t.primary).into()
        } else if installed {
            row![
                text("Installed").size(13).color(t.success),
                button(ui::label("Remove")).style(ui::ghost).on_press(Message::Remove(pack.folder.clone())),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into()
        } else {
            button(ui::label(format!("Install · {}", size(pack.download_size())))).style(ui::primary).on_press(Message::Install(pack.id.clone())).into()
        };
        let kinds = pack.kinds.iter().map(|kind| paperloom_common::widgets::entry(kind).map_or(kind.as_str(), |entry| entry.name)).collect::<Vec<_>>().join(" · ");
        container(
            column![
                preview,
                row![text(pack.name.as_str()).size(15).color(t.foreground), Space::new().width(Length::Fill), text(pack.author.as_str()).size(12).color(t.muted_foreground)].align_y(Alignment::Center),
                text(kinds).size(12).color(t.primary),
                text(pack.description.as_str()).size(12).color(t.muted_foreground),
                action,
            ]
            .spacing(8),
        )
        .padding(10)
        .width(Length::Fixed(320.0))
        .style(move |_theme| container::Style {
            background: Some(Background::Color(t.surface)),
            border: Border { color: t.border, width: 1.0, radius: 6.0.into() },
            ..container::Style::default()
        })
        .into()
    }
}

fn size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    }
}
