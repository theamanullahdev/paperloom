//! The Library page: every imported wallpaper, in three views —
//!
//! - **All**: the library minus anything hidden.
//! - **Collections**: named groups. A collection only *links* wallpapers
//!   (by id), so one wallpaper can be in many collections and nothing is
//!   ever copied; opening a collection shows its wallpapers.
//! - **Hidden**: wallpapers and collections the user hid instead of deleting.
//!
//! Wallpapers show as a grid of cards or a list of rows, each in three sizes
//! (`Settings::library_view`). A wallpaper's ⋮ opens one dialog with every
//! action: set, preview, collections, hide, remove.

use std::collections::HashMap;
use std::time::Duration;

use iced::widget::image::Allocation;
use iced::widget::{
    button, column, container, grid, image, mouse_area, opaque, row, scrollable, stack, svg, text, text_input, toggler, Space,
};
use iced::{Alignment, Background, Border, Color, Element, Length, Shadow, Subscription, Task};

use paperloom_common::{Collection, Library, LibraryEntry, LibraryLayout, LibraryView, Settings, ViewSize};

use crate::{icons, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    All,
    Collections,
    /// Rotation and schedule.
    Rotation,
    /// One collection's wallpapers.
    Collection(u64),
    Hidden,
}

/// The ⋮ dialog for one wallpaper.
struct Actions {
    entry: u64,
    confirm_remove: bool,
    new_collection: String,
    tags: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Added,
    Newest,
    Name,
    FavoritesFirst,
}

const SORTS: [Sort; 4] = [Sort::Added, Sort::Newest, Sort::Name, Sort::FavoritesFirst];

impl std::fmt::Display for Sort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Sort::Added => "Oldest first",
            Sort::Newest => "Newest first",
            Sort::Name => "Name A–Z",
            Sort::FavoritesFirst => "Favorites first",
        })
    }
}

/// Creating, renaming or deleting a collection.
enum CollectionEdit {
    New(String),
    Rename(u64, String),
    ConfirmDelete(u64),
}

pub struct State {
    library: Library,
    search: String,
    status: Option<String>,
    tab: Tab,
    actions: Option<Actions>,
    collection_edit: Option<CollectionEdit>,
    /// Collections tab: showing the automatic groups instead of the user's.
    auto_mode: bool,
    /// The automatic group that's open, by name.
    auto_open: Option<String>,
    /// The wallpaper under the cursor — the only one that animates.
    hovered: Option<u64>,
    /// Frames elapsed since the hover began (see `ping_pong`).
    preview_tick: usize,
    /// GPU-resident preview frames for the hovered wallpaper, by id. iced
    /// uploads images to the GPU asynchronously, so a frame that has never
    /// been drawn shows nothing for a frame or two — exactly the black
    /// flicker when animating. Holding an `Allocation` per frame guarantees
    /// it draws immediately; playback only starts once every frame is ready.
    preview_cache: HashMap<u64, Vec<Option<Allocation>>>,
    /// The Stop button was clicked and the choice is showing.
    confirm_stop: bool,
    sort: Sort,
    optimizer: crate::optimizer::Optimizer,
    rotation: super::rotation::State,
}

#[derive(Debug, Clone)]
pub enum Message {
    SearchChanged(String),
    ImportClicked,
    TabSelected(Tab),
    LayoutSelected(LibraryLayout),
    SizeSelected(ViewSize),
    // --- One wallpaper
    OpenActions(u64),
    CloseActions,
    SetAsWallpaper(u64),
    OptimizeTick,
    Rotation(super::rotation::Message),
    /// The engine answered (or didn't): the text to show.
    PlayFinished(Result<String, String>),
    OpenPreview(u64),
    SetHidden(u64, bool),
    SetInCollection { collection: u64, entry: u64, member: bool },
    ActionsCollectionName(String),
    ActionsTags(String),
    SaveTags(u64),
    ToggleFavorite(u64, bool),
    SortSelected(Sort),
    /// Creates a collection named in the dialog, with this wallpaper in it.
    CreateCollectionWith(u64),
    RemoveClicked,
    ConfirmRemove(u64),
    CancelRemove,
    // --- Collections
    NewCollection,
    RenameCollection(u64),
    CollectionNameChanged(String),
    /// Collections tab: automatic groups (true) or the user's own.
    AutoMode(bool),
    OpenAuto(Option<String>),
    /// Turns an automatic group into a real collection.
    SaveAuto(String),
    SaveCollectionEdit,
    CancelCollectionEdit,
    SetCollectionHidden(u64, bool),
    DeleteCollection(u64),
    ConfirmDeleteCollection(u64),
    // --- Stop
    /// Opens the "stop wallpaper / stop and quit" choice.
    StopClicked,
    StopWallpaper,
    StopAndQuit,
    CancelStop,
    // --- Hover previews
    PreviewTick,
    Hovered(u64),
    Unhovered(u64),
    FrameAllocated { entry: u64, frame: usize, result: Result<Allocation, iced::widget::image::Error> },
}

impl State {
    pub fn new() -> Self {
        let state = Self {
            library: Library::load(),
            search: String::new(),
            status: None,
            tab: Tab::All,
            actions: None,
            collection_edit: None,
            auto_mode: false,
            auto_open: None,
            hovered: None,
            preview_tick: 0,
            preview_cache: HashMap::new(),
            confirm_stop: false,
            sort: Sort::default(),
            optimizer: crate::optimizer::Optimizer::default(),
            rotation: super::rotation::State::new(),
        };
        state.queue_optimizations();
        state
    }

    /// Hands wallpapers waiting for the background optimizer to its worker.
    fn queue_optimizations(&self) {
        for (id, path) in self.library.pending_optimizations() {
            self.optimizer.enqueue(id, path);
        }
    }

    /// Ticks only while the library tab is on screen *and* a wallpaper with
    /// motion frames is hovered — at rest (the normal case) this page
    /// schedules nothing, so an open-but-idle library costs nothing.
    pub fn subscription(&self, active: bool) -> Subscription<Message> {
        let animating = self.hovered.is_some_and(|id| self.preview_ready(id));
        let optimizing = self.optimizer.busy();
        Subscription::batch([
            if active && animating { iced::time::every(Duration::from_millis(100)).map(|_| Message::PreviewTick) } else { Subscription::none() },
            // Only while a wallpaper is being optimized (after an import).
            if optimizing { iced::time::every(Duration::from_secs(1)).map(|_| Message::OptimizeTick) } else { Subscription::none() },
        ])
    }

    fn preview_ready(&self, id: u64) -> bool {
        self.preview_cache.get(&id).is_some_and(|frames| frames.len() > 1 && frames.iter().all(Option::is_some))
    }

    /// Reports a failed save; success clears nothing (other messages stay).
    fn saved(&mut self, result: std::io::Result<()>) {
        if let Err(err) = result {
            self.status = Some(format!("Couldn't save the library: {err}"));
        }
    }

    pub fn update(&mut self, settings: &mut Settings, message: Message) -> Task<Message> {
        match message {
            Message::SearchChanged(value) => self.search = value,
            Message::Rotation(message) => self.rotation.update(message, &self.library.collections),
            Message::OptimizeTick => {
                for (id, outcome) in self.optimizer.take_finished() {
                    let result = self.library.finish_optimize(id, outcome);
                    self.saved(result);
                    self.preview_cache.remove(&id);
                }
            }
            Message::ImportClicked => self.import(settings),
            Message::TabSelected(tab) => {
                self.tab = tab;
                self.collection_edit = None;
                self.auto_open = None;
            }
            Message::AutoMode(auto) => {
                self.auto_mode = auto;
                self.auto_open = None;
            }
            Message::OpenAuto(name) => self.auto_open = name,
            Message::SaveAuto(name) => {
                let visible: Vec<&LibraryEntry> = self.library.entries.iter().filter(|entry| !entry.hidden).collect();
                if let Some(group) = super::auto_groups::groups(&visible).into_iter().find(|group| group.name == name) {
                    match self.library.create_collection(&group.name) {
                        Ok(id) => {
                            for member in group.members {
                                let result = self.library.set_in_collection(id, member, true);
                                self.saved(result);
                            }
                            self.auto_mode = false;
                            self.auto_open = None;
                            self.tab = Tab::Collection(id);
                        }
                        Err(err) => self.status = Some(format!("Couldn't save the collection: {err}")),
                    }
                }
            }
            Message::LayoutSelected(layout) => {
                settings.library_view.layout = layout;
                let _ = paperloom_common::save(settings);
            }
            Message::SizeSelected(size) => {
                settings.library_view.size = size;
                let _ = paperloom_common::save(settings);
            }

            Message::OpenActions(entry) => {
                let tags = self.library.entry(entry).map(|found| found.tags.join(", ")).unwrap_or_default();
                self.actions = Some(Actions { entry, confirm_remove: false, new_collection: String::new(), tags });
            }
            Message::ActionsTags(text) => {
                if let Some(actions) = &mut self.actions {
                    actions.tags = text;
                }
            }
            Message::SaveTags(id) => {
                if let Some(tags) = self.actions.as_ref().map(|actions| actions.tags.clone()) {
                    let result = self.library.set_tags(id, &tags);
                    self.saved(result);
                }
            }
            Message::ToggleFavorite(id, favorite) => {
                let result = self.library.set_favorite(id, favorite);
                self.saved(result);
            }
            Message::SortSelected(sort) => self.sort = sort,
            Message::CloseActions => self.actions = None,
            Message::SetAsWallpaper(id) => {
                self.actions = None;
                if let Some(entry) = self.library.entry(id) {
                    let (path, name) = (entry.path.clone(), entry.display_name.clone());
                    self.status = Some(format!("Starting “{name}”…"));
                    // Off the UI thread: a first start can take a while.
                    return Task::perform(
                        crate::pages::online::blocking(move || crate::engine_client::play(&path).map(|()| format!("Now playing: {name}")).map_err(|err| err.to_string())),
                        Message::PlayFinished,
                    );
                }
            }
            Message::PlayFinished(result) => {
                self.status = Some(match result {
                    Ok(text) => text,
                    Err(err) => format!("Couldn't start playback: {err}"),
                });
            }
            Message::OpenPreview(id) => {
                if let Some(entry) = self.library.entry(id) {
                    if let Err(err) = crate::engine_client::preview(&entry.path) {
                        self.status = Some(format!("Couldn't open the preview: {err}"));
                    }
                }
            }
            Message::SetHidden(id, hidden) => {
                let result = self.library.set_hidden(id, hidden);
                self.saved(result);
                if let Some(entry) = self.library.entry(id) {
                    self.status = Some(if hidden {
                        format!("Hid “{}” — find it under Hidden.", entry.display_name)
                    } else {
                        format!("“{}” is back in your library.", entry.display_name)
                    });
                }
                self.actions = None;
            }
            Message::SetInCollection { collection, entry, member } => {
                let result = self.library.set_in_collection(collection, entry, member);
                self.saved(result);
            }
            Message::ActionsCollectionName(name) => {
                if let Some(actions) = &mut self.actions {
                    actions.new_collection = name;
                }
            }
            Message::CreateCollectionWith(entry) => {
                let name = self.actions.as_ref().map(|actions| actions.new_collection.trim().to_string()).unwrap_or_default();
                if !name.is_empty() {
                    match self.library.create_collection(&name) {
                        Ok(collection) => {
                            let result = self.library.set_in_collection(collection, entry, true);
                            self.saved(result);
                        }
                        Err(err) => self.saved(Err(err)),
                    }
                    if let Some(actions) = &mut self.actions {
                        actions.new_collection.clear();
                    }
                }
            }
            Message::RemoveClicked => {
                if let Some(actions) = &mut self.actions {
                    actions.confirm_remove = true;
                }
            }
            Message::CancelRemove => {
                if let Some(actions) = &mut self.actions {
                    actions.confirm_remove = false;
                }
            }
            Message::ConfirmRemove(id) => {
                self.actions = None;
                self.hovered = None;
                self.preview_cache.clear();
                self.status = match self.library.remove(id) {
                    Ok(()) => None,
                    Err(err) => Some(format!("Couldn't remove that wallpaper: {err}")),
                };
            }

            Message::NewCollection => self.collection_edit = Some(CollectionEdit::New(String::new())),
            Message::RenameCollection(id) => {
                let name = self.library.collection(id).map(|c| c.name.clone()).unwrap_or_default();
                self.collection_edit = Some(CollectionEdit::Rename(id, name));
            }
            Message::CollectionNameChanged(value) => match &mut self.collection_edit {
                Some(CollectionEdit::New(name)) | Some(CollectionEdit::Rename(_, name)) => *name = value,
                _ => {}
            },
            Message::SaveCollectionEdit => match self.collection_edit.take() {
                Some(CollectionEdit::New(name)) if !name.trim().is_empty() => match self.library.create_collection(&name) {
                    Ok(id) => self.tab = Tab::Collection(id),
                    Err(err) => self.saved(Err(err)),
                },
                Some(CollectionEdit::Rename(id, name)) if !name.trim().is_empty() => {
                    let result = self.library.rename_collection(id, &name);
                    self.saved(result);
                }
                other => self.collection_edit = other,
            },
            Message::CancelCollectionEdit => self.collection_edit = None,
            Message::SetCollectionHidden(id, hidden) => {
                let result = self.library.set_collection_hidden(id, hidden);
                self.saved(result);
                if hidden && self.tab == Tab::Collection(id) {
                    self.tab = Tab::Collections;
                }
            }
            Message::DeleteCollection(id) => self.collection_edit = Some(CollectionEdit::ConfirmDelete(id)),
            Message::ConfirmDeleteCollection(id) => {
                self.collection_edit = None;
                let result = self.library.delete_collection(id);
                self.saved(result);
                if self.tab == Tab::Collection(id) {
                    self.tab = Tab::Collections;
                }
            }

            Message::StopClicked => self.confirm_stop = true,
            Message::CancelStop => self.confirm_stop = false,
            // The engine exits on Stop, which brings the normal desktop
            // background back (see paperloom-engine's player).
            Message::StopWallpaper => {
                self.confirm_stop = false;
                self.status = match crate::engine_client::stop() {
                    Ok(()) => Some("Wallpaper stopped — your normal desktop background is back.".to_string()),
                    Err(err) => Some(format!("Couldn't stop the wallpaper: {err}")),
                };
            }
            Message::StopAndQuit => {
                let _ = crate::engine_client::stop();
                return iced::exit();
            }

            Message::PreviewTick => self.preview_tick = self.preview_tick.wrapping_add(1),
            Message::Hovered(id) => {
                // Restart from the poster frame so playback begins exactly
                // where the static image already was — no visible jump.
                self.hovered = Some(id);
                self.preview_tick = 0;
                let Some(entry) = self.library.entry(id) else { return Task::none() };
                if entry.preview_frames.len() > 1 && !self.preview_cache.contains_key(&id) {
                    // Keep GPU memory bounded: only the hovered wallpaper
                    // holds its frames.
                    self.preview_cache.clear();
                    self.preview_cache.insert(id, vec![None; entry.preview_frames.len()]);
                    return Task::batch(entry.preview_frames.iter().enumerate().map(|(frame, path)| {
                        iced::widget::image::allocate(image::Handle::from_path(path))
                            .map(move |result| Message::FrameAllocated { entry: id, frame, result })
                    }));
                }
            }
            Message::Unhovered(id) => {
                if self.hovered == Some(id) {
                    self.hovered = None;
                }
            }
            Message::FrameAllocated { entry, frame, result } => {
                if let (Ok(allocation), Some(frames)) = (result, self.preview_cache.get_mut(&entry)) {
                    if let Some(slot) = frames.get_mut(frame) {
                        *slot = Some(allocation);
                    }
                }
            }
        }
        Task::none()
    }

    fn import(&mut self, settings: &Settings) {
        let picked = rfd::FileDialog::new().set_title("Import wallpapers").add_filter("MP4 video", &["mp4"]).pick_files();
        let Some(paths) = picked else { return };
        let total = paths.len();
        let mut failed = Vec::new();
        for path in &paths {
            if let Err(err) = self.library.import(path, settings.import_mode) {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                failed.push(format!("{name} ({err})"));
            }
        }
        // Imported while looking at a collection: put them in it.
        if let Tab::Collection(collection) = self.tab {
            let new: Vec<u64> = self.library.entries.iter().rev().take(total - failed.len()).map(|entry| entry.id).collect();
            for id in new {
                let result = self.library.set_in_collection(collection, id, true);
                self.saved(result);
            }
        }
        self.preview_cache.clear();
        self.queue_optimizations();
        self.status = if failed.is_empty() {
            None
        } else if failed.len() == total {
            Some(format!("Couldn't import: {}", failed.join(", ")))
        } else {
            Some(format!("Imported {}/{total}. Failed: {}", total - failed.len(), failed.join(", ")))
        };
    }

    /// Imports a finished online download, titled and tagged with its
    /// origin so the online browser can mark it "In library".
    pub fn import_downloaded(&mut self, settings: &Settings, job: &paperloom_fetch::Job) -> Result<(), String> {
        self.preview_cache.clear();
        self.hovered = None;
        let result = self
            .library
            .import_named(&job.final_path(), settings.import_mode, &job.title, &job.id)
            .map_err(|err| format!("Couldn't add \"{}\" to your library: {err}", job.title));
        self.queue_optimizations();
        result
    }

    pub fn set_status(&mut self, message: String) {
        self.status = Some(message);
    }

    pub fn has_source(&self, source_id: &str) -> bool {
        self.library.contains_source(source_id)
    }

    pub fn tab(&self) -> Tab {
        self.tab
    }

    pub fn library(&self) -> &Library {
        &self.library
    }

    /// Shows the whole library filtered to names containing `filter` (set
    /// from the global search; "" shows everything).
    pub fn set_filter(&mut self, filter: String) {
        self.search = filter;
        self.tab = Tab::All;
    }

    /// Names of the wallpapers that aren't hidden, for the global search.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.library.entries.iter().filter(|entry| !entry.hidden).map(|entry| entry.display_name.as_str())
    }

    pub fn header<'a>(&'a self) -> Element<'a, Message> {
        row![button(crate::ui::label("Import wallpapers...")).style(button::primary).on_press(Message::ImportClicked), self.stop_controls()]
            .spacing(8)
            .align_y(Alignment::Center)
            .wrap()
            .vertical_spacing(8)
            .into()
    }

    /// "Stop wallpaper", which turns into the choice between stopping only
    /// the wallpaper (Paperloom stays open) and stopping it and quitting.
    fn stop_controls<'a>(&self) -> Element<'a, Message> {
        if !self.confirm_stop {
            return button(crate::ui::label("Stop wallpaper")).style(button::secondary).on_press(Message::StopClicked).into();
        }
        row![
            button(crate::ui::label("Stop wallpaper only")).style(button::secondary).on_press(Message::StopWallpaper),
            button(crate::ui::label("Stop and quit Paperloom")).style(button::danger).on_press(Message::StopAndQuit),
            button(crate::ui::label("Cancel")).style(button::text).on_press(Message::CancelStop),
        ]
        .spacing(6)
        .wrap()
        .vertical_spacing(6)
        .into()
    }

    pub fn body<'a>(&'a self, view: LibraryView, t: theme::Tokens) -> Element<'a, Message> {
        let mut page = column![self.toolbar(view, t)].spacing(14);
        if let Some(status) = &self.status {
            page = page.push(text(status.clone()).size(13).color(t.muted_foreground));
        }
        if !self.search.is_empty() {
            page = page.push(
                row![
                    text(format!("Wallpapers matching “{}”", self.search)).color(t.foreground),
                    button(crate::ui::label("Show all").size(12)).style(button::text).padding([2, 6]).on_press(Message::SearchChanged(String::new())),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .wrap(),
            );
        }

        let content: Element<'a, Message> = match self.tab {
            Tab::All => {
                let entries = self.visible(|entry| !entry.hidden);
                let empty = if self.library.entries.is_empty() {
                    "No wallpapers yet. Import an MP4, or download some from Online."
                } else {
                    "No wallpapers match."
                };
                self.wallpapers(entries, view, empty, t)
            }
            Tab::Collections => self.collections_page(view, t),
            Tab::Rotation => self.rotation.view(&self.library.collections, t).map(Message::Rotation),
            Tab::Collection(id) => self.collection_page(id, view, t),
            Tab::Hidden => self.hidden_page(view, t),
        };
        page = page.push(content);
        // Full height, so the dialog's backdrop covers the page, not just its content.
        let page: Element<'a, Message> = crate::ui::scroll_page(page).height(Length::Fill).into();

        // The ⋮ dialog floats over the page; clicking outside closes it.
        match self.actions.as_ref().and_then(|actions| self.library.entry(actions.entry).map(|entry| (actions, entry))) {
            Some((actions, entry)) => stack![
                page,
                mouse_area(container(Space::new()).width(Length::Fill).height(Length::Fill).style(|_theme| container::Style {
                    background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.35))),
                    ..container::Style::default()
                }))
                .on_press(Message::CloseActions),
                container(opaque(self.actions_dialog(actions, entry, t))).center(Length::Fill).padding(12),
            ]
            .into(),
            None => page,
        }
    }

    /// Entries passing `keep` and the search box.
    fn visible(&self, keep: impl Fn(&LibraryEntry) -> bool) -> Vec<&LibraryEntry> {
        let query = self.search.to_lowercase();
        let mut entries: Vec<&LibraryEntry> = self
            .library
            .entries
            .iter()
            .filter(|entry| keep(entry) && (query.is_empty() || entry.display_name.to_lowercase().contains(&query) || entry.tags.iter().any(|tag| tag.contains(query.trim_start_matches('#')))))
            .collect();
        match self.sort {
            Sort::Added => {}
            Sort::Newest => entries.sort_by_key(|entry| std::cmp::Reverse((entry.added, entry.id))),
            Sort::Name => entries.sort_by_key(|entry| entry.display_name.to_lowercase()),
            Sort::FavoritesFirst => entries.sort_by_key(|entry| !entry.favorite),
        }
        entries
    }

    /// Tabs on the left, view switches on the right.
    fn toolbar<'a>(&'a self, view: LibraryView, t: theme::Tokens) -> Element<'a, Message> {
        let shown = self.library.entries.iter().filter(|entry| !entry.hidden).count();
        let collections = self.library.collections.iter().filter(|collection| !collection.hidden).count();
        let hidden = self.library.entries.iter().filter(|entry| entry.hidden).count() + self.library.collections.len() - collections;
        let in_collections = matches!(self.tab, Tab::Collections | Tab::Collection(_));

        let tabs = segmented(
            vec![
                (format!("All  {shown}"), self.tab == Tab::All, Message::TabSelected(Tab::All)),
                (format!("Collections  {collections}"), in_collections, Message::TabSelected(Tab::Collections)),
                ("Rotation".to_string(), self.tab == Tab::Rotation, Message::TabSelected(Tab::Rotation)),
                (format!("Hidden  {hidden}"), self.tab == Tab::Hidden, Message::TabSelected(Tab::Hidden)),
            ],
            t,
        );

        let layout = icon_segmented(
            vec![
                (icons::grid(), "Grid", view.layout == LibraryLayout::Grid, Message::LayoutSelected(LibraryLayout::Grid)),
                (icons::list(), "List", view.layout == LibraryLayout::List, Message::LayoutSelected(LibraryLayout::List)),
            ],
            t,
        );
        let size = segmented(
            vec![
                ("S".to_string(), view.size == ViewSize::Small, Message::SizeSelected(ViewSize::Small)),
                ("M".to_string(), view.size == ViewSize::Medium, Message::SizeSelected(ViewSize::Medium)),
                ("L".to_string(), view.size == ViewSize::Large, Message::SizeSelected(ViewSize::Large)),
            ],
            t,
        );

        // No spacer: in a wrapping row it would push the view switches onto
        // their own line.
        let sort = iced::widget::pick_list(SORTS, Some(self.sort), Message::SortSelected);
        row![tabs, Space::new().width(12), layout, size, sort]
            .spacing(8)
            .align_y(Alignment::Center)
            .wrap()
            .vertical_spacing(8)
            .into()
    }

    // --- Wallpapers, in the chosen view -----------------------------------

    fn wallpapers<'a>(&'a self, entries: Vec<&'a LibraryEntry>, view: LibraryView, empty: &'static str, t: theme::Tokens) -> Element<'a, Message> {
        if entries.is_empty() {
            return container(text(empty).color(t.muted_foreground)).padding(24).into();
        }
        match view.layout {
            LibraryLayout::Grid => {
                let (column_width, chrome) = match view.size {
                    ViewSize::Small => (170.0, 34.0),
                    ViewSize::Medium => (260.0, 76.0),
                    ViewSize::Large => (400.0, 80.0),
                };
                grid::Grid::with_children(entries.into_iter().map(|entry| self.card(entry, view.size, t)))
                    .fluid(column_width)
                    .spacing(if view.size == ViewSize::Small { 10 } else { 16 })
                    .height(grid::aspect_ratio(super::gallery::card_ratio_for(column_width, chrome), 1.0))
                    .into()
            }
            LibraryLayout::List => {
                let spacing = if view.size == ViewSize::Small { 4 } else { 8 };
                column(entries.into_iter().map(|entry| self.list_row(entry, view.size, t))).spacing(spacing).into()
            }
        }
    }

    /// The hover-animated poster for a wallpaper.
    fn poster<'a>(&self, entry: &'a LibraryEntry, t: theme::Tokens) -> Element<'a, Message> {
        let tick = if self.hovered == Some(entry.id) && self.preview_ready(entry.id) { self.preview_tick } else { 0 };
        let frame = if tick == 0 { entry.thumbnail.as_ref() } else { entry.preview_frames.get(ping_pong(tick, entry.preview_frames.len())) };
        match frame {
            Some(path) => image(image::Handle::from_path(path)).content_fit(iced::ContentFit::Cover).width(Length::Fill).height(Length::Fill).into(),
            None => container(svg(icons::film()).width(28).height(28).style(move |_theme, _status| svg::Style { color: Some(t.muted_foreground) }))
                .center(Length::Fill)
                .style(move |_theme| container::Style { background: Some(Background::Color(t.muted)), ..container::Style::default() })
                .into(),
        }
    }

    /// Hover (animates the poster) and double-click (full-screen preview).
    fn interactive<'a>(&self, entry: &LibraryEntry, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
        mouse_area(content).on_enter(Message::Hovered(entry.id)).on_exit(Message::Unhovered(entry.id)).on_double_click(Message::OpenPreview(entry.id)).into()
    }

    fn card<'a>(&self, entry: &'a LibraryEntry, size: ViewSize, t: theme::Tokens) -> Element<'a, Message> {
        let menu = container(kebab(entry.id)).padding(6).align_x(Alignment::End).width(Length::Fill);
        let status: Element<'a, Message> = match self.optimizer.progress(entry.id) {
            Some(percent) => container(
                container(text(if percent == 0 { "Optimizing…".to_string() } else { format!("Optimizing {percent}%") }).size(11).color(Color::WHITE)).padding([2, 6]).style(|_theme| container::Style {
                    background: Some(Background::Color(Color { a: 0.6, ..Color::BLACK })),
                    border: Border { radius: 6.0.into(), ..Border::default() },
                    ..container::Style::default()
                }),
            )
            .padding(6)
            .align_y(iced::alignment::Vertical::Bottom)
            .height(Length::Fill)
            .into(),
            None => Space::new().into(),
        };
        let poster = container(stack![self.poster(entry, t), status, menu]).clip(true).width(Length::Fill).height(Length::Fill);

        let name = text(if entry.favorite { format!("★ {}", entry.display_name) } else { entry.display_name.clone() }).size(if size == ViewSize::Small { 12 } else { 13 }).color(t.foreground).wrapping(text::Wrapping::None);
        let chrome: Element<'a, Message> = match size {
            ViewSize::Small => row![container(name).width(Length::Fill).clip(true), play_button(entry.id, t)].spacing(6).align_y(Alignment::Center).padding([6, 6]).into(),
            ViewSize::Medium | ViewSize::Large => column![
                container(name).clip(true),
                button(crate::ui::label("Set as wallpaper").size(12)).style(button::primary).width(Length::Fill).on_press(Message::SetAsWallpaper(entry.id)),
            ]
            .spacing(8)
            .padding([8, 6])
            .into(),
        };

        self.interactive(entry, surface(column![poster, chrome].width(Length::Fill).height(Length::Fill), 12.0, t))
    }

    fn list_row<'a>(&self, entry: &'a LibraryEntry, size: ViewSize, t: theme::Tokens) -> Element<'a, Message> {
        let name = text(entry.display_name.clone()).color(t.foreground);
        let set = button(crate::ui::label("Set as wallpaper").size(12)).style(button::primary).padding([5, 10]).on_press(Message::SetAsWallpaper(entry.id));
        let collections: Vec<&str> = self.library.collections_of(entry.id).map(|collection| collection.name.as_str()).collect();

        let content: Element<'a, Message> = match size {
            // Just the name.
            ViewSize::Small => row![name.size(13).width(Length::Fill), play_button(entry.id, t), kebab_plain(entry.id, t)]
                .spacing(8)
                .align_y(Alignment::Center)
                .padding([4, 10])
                .into(),
            ViewSize::Medium => row![
                container(self.poster(entry, t)).width(Length::Fixed(112.0)).height(Length::Fixed(63.0)).clip(true),
                column![name.size(14), text(subtitle(entry, &collections)).size(12).color(t.muted_foreground)].spacing(4).width(Length::Fill),
                set,
                kebab_plain(entry.id, t),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .padding(8)
            .into(),
            ViewSize::Large => {
                let mut details = column![name.size(16)].spacing(4).width(Length::Fill);
                details = details.push(detail("File", entry.path.display().to_string(), t));
                details = details.push(detail("Size", file_size(entry), t));
                details = details.push(detail("Stored", mode_label(entry), t));
                if entry.added > 0 {
                    details = details.push(detail("Added", format_date(entry.added), t));
                }
                details = details.push(detail("Collections", if collections.is_empty() { "none".into() } else { collections.join(", ") }, t));
                details = details.push(row![set, kebab_plain(entry.id, t)].spacing(8).align_y(Alignment::Center));
                row![container(self.poster(entry, t)).width(Length::Fixed(256.0)).height(Length::Fixed(144.0)).clip(true), details]
                    .spacing(16)
                    .padding(10)
                    .into()
            }
        };
        self.interactive(entry, surface(content, if size == ViewSize::Small { 8.0 } else { 10.0 }, t))
    }

    // --- Collections -------------------------------------------------------

    fn collections_page<'a>(&'a self, view: LibraryView, t: theme::Tokens) -> Element<'a, Message> {
        let modes = segmented(
            vec![("Custom".to_string(), !self.auto_mode, Message::AutoMode(false)), ("Auto".to_string(), self.auto_mode, Message::AutoMode(true))],
            t,
        );
        if self.auto_mode {
            return column![modes, self.auto_page(view, t)].spacing(14).into();
        }
        let mut page = column![modes, self.new_collection_bar(t)].spacing(14);
        let shown: Vec<&Collection> = self.library.collections.iter().filter(|collection| !collection.hidden).collect();
        if shown.is_empty() {
            page = page.push(
                text("No collections yet. Make one here, or from any wallpaper's ⋮ menu. A wallpaper can be in as many collections as you like; nothing is copied.")
                    .color(t.muted_foreground),
            );
        } else {
            page = page.push(self.collection_grid(shown, t));
        }
        page.into()
    }

    fn new_collection_bar<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        match &self.collection_edit {
            Some(CollectionEdit::New(name)) => row![
                text_input("Collection name", name).on_input(Message::CollectionNameChanged).on_submit(Message::SaveCollectionEdit).padding(8).width(Length::Fixed(260.0)),
                button(crate::ui::label("Create")).on_press_maybe((!name.trim().is_empty()).then_some(Message::SaveCollectionEdit)),
                button(crate::ui::label("Cancel")).style(button::text).on_press(Message::CancelCollectionEdit),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .wrap()
            .vertical_spacing(8)
            .into(),
            _ => row![
                button(crate::ui::label("New collection")).style(button::secondary).on_press(Message::NewCollection),
                text("Group wallpapers by mood, season, game… A wallpaper can be in several.").size(12).color(t.muted_foreground),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .wrap()
            .vertical_spacing(6)
            .into(),
        }
    }

    fn collection_grid<'a>(&'a self, collections: Vec<&'a Collection>, t: theme::Tokens) -> Element<'a, Message> {
        grid::Grid::with_children(collections.into_iter().map(|collection| self.collection_card(collection, t)))
            .fluid(240.0)
            .spacing(16)
            .height(grid::aspect_ratio(super::gallery::card_ratio_for(240.0, 52.0), 1.0))
            .into()
    }

    /// A cover of up to four of its wallpapers, its name and size.
    fn collection_card<'a>(&'a self, collection: &'a Collection, t: theme::Tokens) -> Element<'a, Message> {
        self.group_card(&collection.name, &collection.members, Message::TabSelected(Tab::Collection(collection.id)), t)
    }

    fn group_card<'a>(&'a self, name: &str, members: &[u64], on_press: Message, t: theme::Tokens) -> Element<'a, Message> {
        let posters: Vec<&std::path::PathBuf> =
            members.iter().filter_map(|id| self.library.entry(*id)).filter_map(|entry| entry.thumbnail.as_ref()).take(4).collect();
        let tile = |path: &std::path::PathBuf| -> Element<'a, Message> {
            image(image::Handle::from_path(path)).content_fit(iced::ContentFit::Cover).width(Length::Fill).height(Length::Fill).into()
        };
        let cover: Element<'a, Message> = match posters.len() {
            0 => container(svg(icons::folder()).width(32).height(32).style(move |_theme, _status| svg::Style { color: Some(t.muted_foreground) }))
                .center(Length::Fill)
                .style(move |_theme| container::Style { background: Some(Background::Color(t.muted)), ..container::Style::default() })
                .into(),
            1..=3 => tile(posters[0]),
            _ => column![row![tile(posters[0]), tile(posters[1])].spacing(2), row![tile(posters[2]), tile(posters[3])].spacing(2)].spacing(2).into(),
        };
        let count = members.iter().filter(|id| self.library.entry(**id).is_some_and(|entry| !entry.hidden)).count();
        let label = column![
            text(name.to_string()).size(14).color(t.foreground).wrapping(text::Wrapping::None),
            text(match count {
                1 => "1 wallpaper".to_string(),
                n => format!("{n} wallpapers"),
            })
            .size(12)
            .color(t.muted_foreground),
        ]
        .spacing(2)
        .padding([8, 10]);

        button(surface(column![container(cover).clip(true).width(Length::Fill).height(Length::Fill), label].width(Length::Fill).height(Length::Fill), 12.0, t))
            .padding(0)
            .style(|_theme, _status| button::Style::default())
            .on_press(on_press)
            .into()
    }

    /// The automatic groups, or one of them opened.
    fn auto_page<'a>(&'a self, view: LibraryView, t: theme::Tokens) -> Element<'a, Message> {
        let visible: Vec<&LibraryEntry> = self.library.entries.iter().filter(|entry| !entry.hidden).collect();
        let groups = super::auto_groups::groups(&visible);
        if visible.is_empty() {
            return text("Import or download wallpapers and they are sorted here by themselves.").color(t.muted_foreground).into();
        }
        if let Some(open) = self.auto_open.as_ref().and_then(|name| groups.iter().find(|group| &group.name == name)) {
            let back = button(crate::ui::label("All groups").size(13)).style(button::text).padding([4, 6]).on_press(Message::OpenAuto(None));
            let save = button(crate::ui::label("Save as a collection").size(12)).style(button::secondary).padding([4, 10]).on_press(Message::SaveAuto(open.name.clone()));
            let entries = self.visible(|entry| open.members.contains(&entry.id) && !entry.hidden);
            return column![
                back,
                row![text(open.name.clone()).font(theme::DISPLAY_FONT).size(20).color(t.foreground), Space::new().width(Length::Fill), save].spacing(12).align_y(Alignment::Center),
                self.wallpapers(entries, view, "Nothing here.", t),
            ]
            .spacing(12)
            .into();
        }
        let cards: Vec<Element<'a, Message>> = groups.iter().map(|group| self.group_card(&group.name, &group.members, Message::OpenAuto(Some(group.name.clone())), t)).collect();
        column![
            text("Sorted automatically from each wallpaper's title and where it came from. A wallpaper can appear in several groups; nothing is copied.").size(12).color(t.muted_foreground),
            grid::Grid::with_children(cards).fluid(240.0).spacing(16).height(grid::aspect_ratio(super::gallery::card_ratio_for(240.0, 52.0), 1.0)),
        ]
        .spacing(12)
        .into()
    }

    fn collection_page<'a>(&'a self, id: u64, view: LibraryView, t: theme::Tokens) -> Element<'a, Message> {
        let Some(collection) = self.library.collection(id) else {
            return text("That collection no longer exists.").color(t.muted_foreground).into();
        };
        let back = button(
            row![svg(icons::back()).width(16).height(16).style(move |_theme, _status| svg::Style { color: Some(t.foreground) }), crate::ui::label("Collections").size(13)]
                .spacing(4)
                .align_y(Alignment::Center),
        )
        .style(button::text)
        .padding([4, 6])
        .on_press(Message::TabSelected(Tab::Collections));

        let title: Element<'a, Message> = match &self.collection_edit {
            Some(CollectionEdit::Rename(editing, name)) if *editing == id => row![
                text_input("Collection name", name).on_input(Message::CollectionNameChanged).on_submit(Message::SaveCollectionEdit).padding(6).width(Length::Fixed(240.0)),
                button(crate::ui::label("Save")).on_press_maybe((!name.trim().is_empty()).then_some(Message::SaveCollectionEdit)),
                button(crate::ui::label("Cancel")).style(button::text).on_press(Message::CancelCollectionEdit),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .wrap()
            .into(),
            _ => text(collection.name.clone()).font(theme::DISPLAY_FONT).size(20).color(t.foreground).into(),
        };

        let small = |label: &'static str, message: Message| button(crate::ui::label(label).size(12)).style(button::secondary).padding([4, 10]).on_press(message);
        let actions: Element<'a, Message> = match &self.collection_edit {
            Some(CollectionEdit::ConfirmDelete(deleting)) if *deleting == id => row![
                text("Delete this collection? Its wallpapers stay in your library.").size(13).color(t.foreground),
                button(crate::ui::label("Delete collection").size(12)).style(button::danger).padding([4, 10]).on_press(Message::ConfirmDeleteCollection(id)),
                button(crate::ui::label("Cancel").size(12)).style(button::text).padding([4, 10]).on_press(Message::CancelCollectionEdit),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .wrap()
            .vertical_spacing(6)
            .into(),
            _ => row![
                small("Rename", Message::RenameCollection(id)),
                if collection.hidden { small("Unhide", Message::SetCollectionHidden(id, false)) } else { small("Hide", Message::SetCollectionHidden(id, true)) },
                button(crate::ui::label("Delete").size(12)).style(button::danger).padding([4, 10]).on_press(Message::DeleteCollection(id)),
            ]
            .spacing(6)
            .wrap()
            .vertical_spacing(6)
            .into(),
        };

        let entries = self.visible(|entry| collection.members.contains(&entry.id) && !entry.hidden);
        column![
            back,
            row![title, Space::new().width(Length::Fill), actions].spacing(12).align_y(Alignment::Center).wrap().vertical_spacing(8),
            self.wallpapers(entries, view, "Nothing here yet. Use a wallpaper's ⋮ menu → Collections to add it, or import while this collection is open.", t),
        ]
        .spacing(12)
        .into()
    }

    fn hidden_page<'a>(&'a self, view: LibraryView, t: theme::Tokens) -> Element<'a, Message> {
        let entries = self.visible(|entry| entry.hidden);
        let collections: Vec<&Collection> = self.library.collections.iter().filter(|collection| collection.hidden).collect();
        let mut page = column![
            text("Hidden things stay on disk and in their collections; they just don't show up. Unhide them from their ⋮ menu or page.")
                .size(13)
                .color(t.muted_foreground),
            section("Wallpapers", t),
            self.wallpapers(entries, view, "No hidden wallpapers.", t),
            section("Collections", t),
        ]
        .spacing(12);
        page = page.push(if collections.is_empty() {
            Element::from(container(text("No hidden collections.").color(t.muted_foreground)).padding(24))
        } else {
            self.collection_grid(collections, t)
        });
        page.into()
    }

    // --- The ⋮ dialog ------------------------------------------------------

    fn actions_dialog<'a>(&'a self, actions: &'a Actions, entry: &'a LibraryEntry, t: theme::Tokens) -> Element<'a, Message> {
        let id = entry.id;
        let mut body = column![
            row![
                text(entry.display_name.clone()).font(theme::DISPLAY_FONT).size(18).color(t.foreground).width(Length::Fill),
                button(crate::ui::label("Close").size(12)).style(button::text).on_press(Message::CloseActions),
            ]
            .align_y(Alignment::Center),
            container(self.poster(entry, t)).height(Length::Fixed(150.0)).width(Length::Fill).clip(true),
            row![
                button(crate::ui::label("Set as wallpaper")).style(button::primary).on_press(Message::SetAsWallpaper(id)),
                button(crate::ui::label("Preview")).style(button::secondary).on_press(Message::OpenPreview(id)),
            ]
            .spacing(8)
            .wrap()
            .vertical_spacing(6),
            toggler(entry.favorite).label("Favorite ★").on_toggle(move |favorite| Message::ToggleFavorite(id, favorite)),
            row![
                text_input("Tags, separated by commas", &actions.tags).on_input(Message::ActionsTags).on_submit(Message::SaveTags(id)).padding(6).width(Length::Fill),
                button(crate::ui::label("Save tags")).style(button::secondary).on_press(Message::SaveTags(id)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            section("Collections", t),
        ]
        .spacing(10);

        if self.library.collections.is_empty() {
            body = body.push(text("No collections yet — name one below.").size(12).color(t.muted_foreground));
        } else {
            let list = column(self.library.collections.iter().map(|collection| {
                let member = collection.members.contains(&id);
                let label = if collection.hidden { format!("{} (hidden)", collection.name) } else { collection.name.clone() };
                toggler(member)
                    .label(label)
                    .on_toggle(move |member| Message::SetInCollection { collection: collection.id, entry: id, member })
                    .into()
            }))
            .spacing(6);
            body = body.push(scrollable(list).height(Length::Fixed((self.library.collections.len() as f32 * 30.0).min(180.0))));
        }
        let name = actions.new_collection.trim();
        body = body.push(
            row![
                text_input("New collection…", &actions.new_collection)
                    .on_input(Message::ActionsCollectionName)
                    .on_submit(Message::CreateCollectionWith(id))
                    .padding(6)
                    .width(Length::Fill),
                button(crate::ui::label("Add")).style(button::secondary).on_press_maybe((!name.is_empty()).then_some(Message::CreateCollectionWith(id))),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );

        body = body.push(section("Manage", t));
        if actions.confirm_remove {
            let consequence = if entry.mode == paperloom_common::ImportMode::ReferenceInPlace {
                "Removes it from Paperloom. The original file is left where it is."
            } else {
                "Removes it from Paperloom and deletes its copy in the library folder."
            };
            body = body.push(
                column![
                    text(consequence).size(13).color(t.foreground),
                    row![
                        button(crate::ui::label("Remove")).style(button::danger).on_press(Message::ConfirmRemove(id)),
                        button(crate::ui::label("Cancel")).style(button::text).on_press(Message::CancelRemove),
                    ]
                    .spacing(8),
                ]
                .spacing(8),
            );
        } else {
            body = body.push(
                row![
                    if entry.hidden {
                        button(crate::ui::label("Show in library")).style(button::secondary).on_press(Message::SetHidden(id, false))
                    } else {
                        button(crate::ui::label("Hide")).style(button::secondary).on_press(Message::SetHidden(id, true))
                    },
                    button(crate::ui::label("Remove…")).style(button::danger).on_press(Message::RemoveClicked),
                ]
                .spacing(8)
                .wrap()
                .vertical_spacing(6),
            );
        }

        container(scrollable(body))
            .padding(16)
            .max_width(440.0)
            .max_height(620.0)
            .style(move |_theme| container::Style {
                background: Some(Background::Color(t.surface)),
                border: Border { color: t.border, width: 1.0, radius: 14.0.into() },
                shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.4), offset: iced::Vector::new(0.0, 8.0), blur_radius: 28.0 },
                ..container::Style::default()
            })
            .into()
    }
}

// --- Small pieces --------------------------------------------------------

fn segmented<'a>(items: Vec<(String, bool, Message)>, t: theme::Tokens) -> Element<'a, Message> {
    crate::ui::segmented(items, t)
}

fn icon_segmented<'a>(items: Vec<(svg::Handle, &'static str, bool, Message)>, t: theme::Tokens) -> Element<'a, Message> {
    let buttons = items.into_iter().map(|(icon, label, active, message)| {
        let color = if active { t.primary } else { t.foreground };
        let content = row![svg(icon).width(15).height(15).style(move |_theme, _status| svg::Style { color: Some(color) }), crate::ui::label(label).size(13)]
            .spacing(6)
            .align_y(Alignment::Center);
        crate::ui::segment(content.into(), active, message, t)
    });
    crate::ui::segment_bar(row(buttons).spacing(2), t)
}

/// Card/row background shared by every view.
fn surface<'a>(content: impl Into<Element<'a, Message>>, radius: f32, t: theme::Tokens) -> Element<'a, Message> {
    container(content)
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(t.surface)),
            border: Border { color: t.border, width: 1.0, radius: radius.into() },
            shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.12), offset: iced::Vector::new(0.0, 2.0), blur_radius: 6.0 },
            ..container::Style::default()
        })
        .into()
}

/// ⋮ over a poster (white on a dark chip, readable on any frame).
fn kebab<'a>(id: u64) -> Element<'a, Message> {
    button(svg(icons::ellipsis()).width(16).height(16).style(|_theme, _status| svg::Style { color: Some(Color::WHITE) }))
        .style(move |_theme, status| button::Style {
            background: Some(Background::Color(Color { a: if matches!(status, button::Status::Hovered | button::Status::Pressed) { 0.7 } else { 0.45 }, ..Color::BLACK })),
            text_color: Color::WHITE,
            border: Border { radius: 6.0.into(), ..Border::default() },
            shadow: Shadow::default(),
            snap: false,
        })
        .padding(4)
        .on_press(Message::OpenActions(id))
        .into()
}

/// ⋮ in a list row.
fn kebab_plain<'a>(id: u64, t: theme::Tokens) -> Element<'a, Message> {
    button(svg(icons::ellipsis()).width(16).height(16).style(move |_theme, _status| svg::Style { color: Some(t.foreground) }))
        .style(button::text)
        .padding(6)
        .on_press(Message::OpenActions(id))
        .into()
}

/// Compact "Set as wallpaper".
fn play_button<'a>(id: u64, t: theme::Tokens) -> Element<'a, Message> {
    button(svg(icons::play()).width(14).height(14).style(move |_theme, _status| svg::Style { color: Some(t.background) }))
        .style(button::primary)
        .padding([4, 8])
        .on_press(Message::SetAsWallpaper(id))
        .into()
}

fn section<'a>(label: &'static str, t: theme::Tokens) -> Element<'a, Message> {
    text(label).size(13).color(t.muted_foreground).into()
}

fn detail<'a>(label: &'static str, value: String, t: theme::Tokens) -> Element<'a, Message> {
    row![text(label).size(12).color(t.muted_foreground).width(Length::Fixed(90.0)), text(value).size(12).color(t.foreground)].into()
}

fn subtitle(entry: &LibraryEntry, collections: &[&str]) -> String {
    let mut parts = vec![file_size(entry)];
    if !collections.is_empty() {
        parts.push(collections.join(", "));
    }
    parts.join(" · ")
}

fn file_size(entry: &LibraryEntry) -> String {
    match entry.path.metadata() {
        Ok(meta) => crate::pages::downloads::megabytes(meta.len()),
        Err(_) => "file missing".into(),
    }
}

fn mode_label(entry: &LibraryEntry) -> String {
    match entry.mode {
        paperloom_common::ImportMode::ReferenceInPlace => "linked where it is (not copied)".into(),
        _ => "copied into the library folder".into(),
    }
}

/// Unix seconds as "YYYY-MM-DD".
fn format_date(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Maps an ever-increasing tick to a frame index that plays forward then
/// backward (0,1,..,n-1,n-2,..,1,0,1,..) — a short clip excerpt isn't
/// loopable, so replaying it backwards avoids a visible jump from the last
/// frame to the first.
fn ping_pong(tick: usize, frames: usize) -> usize {
    if frames <= 1 {
        return 0;
    }
    let period = 2 * (frames - 1);
    let position = tick % period;
    if position < frames {
        position
    } else {
        period - position
    }
}

#[cfg(test)]
mod tests {
    use super::{format_date, ping_pong};

    #[test]
    fn ping_pong_bounces_without_repeating_the_ends() {
        let sequence: Vec<usize> = (0..10).map(|tick| ping_pong(tick, 4)).collect();
        assert_eq!(sequence, [0, 1, 2, 3, 2, 1, 0, 1, 2, 3]);
        assert_eq!(ping_pong(5, 1), 0);
        assert_eq!(ping_pong(5, 0), 0);
    }

    #[test]
    fn formats_dates() {
        assert_eq!(format_date(1_792_281_600), "2026-10-18");
    }
}
