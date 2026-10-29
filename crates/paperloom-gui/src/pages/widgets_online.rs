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
    /// Every look in the catalogue (a stack installs the ones it needs).
    looks: Vec<Pack>,
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
    /// Put this installed stack on the desktop (handled by the widgets page).
    AddStack(String),
}

/// What the library lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Catalog {
    Looks,
    Stacks,
}

enum Status {
    Idle,
    Loading,
    Failed(String),
    Ready { base: String, packs: Vec<Pack>, looks: Vec<Pack>, offline: bool },
}

/// For a look's kinds and id: the widget "Use" would change, and whether it already uses it.
pub type Target<'t> = dyn Fn(&[String], &str) -> Option<(String, bool)> + 't;

pub struct State {
    catalog: Catalog,
    status: Status,
    previews: HashMap<String, image::Handle>,
    installing: HashSet<String>,
    error: Option<String>,
    /// When the catalogue was last asked for (a failed read is tried again).
    asked: std::time::Instant,
}

impl State {
    pub fn new(catalog: Catalog) -> Self {
        Self { catalog, status: Status::Idle, previews: HashMap::new(), installing: HashSet::new(), error: None, asked: std::time::Instant::now() }
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
                let catalog = self.catalog;
                (
                    blocking(
                        move || {
                            let cache = paperloom_common::online_cache_dir().join(if catalog == Catalog::Looks { "looks" } else { "stacks" });
                            let shown = |previews: Vec<(String, Vec<u8>)>| previews.into_iter().filter_map(|(id, bytes)| Some((id, on_wallpaper(&bytes)?))).collect();
                            let pick = |index: looks::Index| if catalog == Catalog::Looks { (index.packs.clone(), index.packs) } else { (index.stacks, index.packs) };
                            match looks::fetch_index() {
                                Ok((index, base)) => {
                                    let (listed, all) = pick(index.clone());
                                    let raw: Vec<(String, Vec<u8>)> = listed
                                        .iter()
                                        .filter_map(|pack| Some((pack.id.clone(), if catalog == Catalog::Looks { looks::fetch_preview(base, pack) } else { looks::fetch_stack_preview(base, pack) }.ok()?)))
                                        .collect();
                                    looks::save_cache(&cache, base, &index, &raw);
                                    Ok(Data { base: base.to_string(), packs: listed, looks: all, previews: shown(raw), offline: false })
                                }
                                // Offline: the library as it was last time.
                                Err(err) => match looks::load_cache(&cache) {
                                    Some((base, index, raw)) => {
                                        let (listed, all) = pick(index);
                                        Ok(Data { base, packs: listed, looks: all, previews: shown(raw), offline: true })
                                    }
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
                self.status = Status::Ready { base: data.base, packs: data.packs, looks: data.looks, offline: data.offline };
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
                let Status::Ready { base, packs, looks: all_looks, .. } = &self.status else { return (Task::none(), false) };
                let Some(pack) = packs.iter().find(|pack| pack.id == id).cloned() else { return (Task::none(), false) };
                let (base, all_looks, catalog) = (base.clone(), all_looks.clone(), self.catalog);
                self.installing.insert(id.clone());
                self.error = None;
                (
                    blocking(
                        move || match catalog {
                            Catalog::Looks => looks::install(&base, &pack, &paperloom_common::skins_dir()),
                            Catalog::Stacks => {
                                // The looks it uses come along (the ones not on this computer yet).
                                for need in &pack.needs {
                                    let have = paperloom_skin::get(need).is_some();
                                    if let (false, Some(look)) = (have, all_looks.iter().find(|look| &look.id == need)) {
                                        looks::install(&base, look, &paperloom_common::skins_dir())?;
                                    }
                                }
                                looks::install_stack(&base, &pack, &paperloom_common::stacks_dir())
                            }
                        },
                        move |result| Message::Done(id.clone(), result),
                    ),
                    false,
                )
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
            Message::Remove(folder) => {
                let catalog = self.catalog;
                (
                    blocking(
                        move || if catalog == Catalog::Looks { looks::remove(&folder, &paperloom_common::skins_dir()) } else { looks::remove_stack(&folder, &paperloom_common::stacks_dir()) },
                        Message::Removed,
                    ),
                    false,
                )
            }
            Message::Use(_) | Message::Add(..) | Message::AddStack(_) => (Task::none(), false),
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
        if self.catalog == Catalog::Stacks {
            return self.stack_card(pack, t);
        }
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

    /// A stack in the library: install it, then put it on the desktop from the same card.
    fn stack_card<'a>(&'a self, pack: &'a Pack, t: theme::Tokens) -> Element<'a, Message> {
        let installed = paperloom_common::stacks::presets().iter().any(|preset| preset.id == pack.id && preset.installed);
        let mut actions: Vec<Element<'a, Message>> = Vec::new();
        if self.installing.contains(&pack.id) {
            actions.push(text("Downloading…").size(13).color(t.primary).into());
        } else if installed {
            actions.push(button(ui::label("Add to desktop").color(ui::on_accent())).style(ui::primary).on_press(Message::AddStack(pack.id.clone())).into());
            actions.push(button(ui::label("Remove")).style(ui::ghost).on_press(Message::Remove(pack.folder.clone())).into());
        } else {
            actions.push(button(ui::label(format!("Install · {}", size(pack.download_size()))).color(ui::on_accent())).style(ui::primary).on_press(Message::Install(pack.id.clone())).into());
        }
        let detail = format!("by {}", pack.author);
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
