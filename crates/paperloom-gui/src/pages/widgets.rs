//! Widgets: the desktop clocks and meters drawn by `paperloom-widgets`, a
//! process of their own (the wallpaper engine never draws them). This page
//! is where they are switched on, added, removed and set up.
//!
//! Every widget is shown the same way everywhere: an equal-size card with the
//! widget drawn on the user's own wallpaper. My widgets is a grid of those
//! cards; a card opens the widget's own page (where it sits, its look, its
//! size, how it behaves). Add a widget lists every kind, with the ones
//! already placed folded under it. Looks is the gallery; Arrange is the
//! whole screen.
//!
//! Everything is written to `widgets.toml`; the widgets process notices the
//! change within a couple of seconds, so there is no other channel to keep
//! in step.

use std::time::Duration;

use iced::widget::{button, column, container, image, row, slider, text, toggler, Space};
use iced::{Alignment, Background, Border, Color, Element, Length, Subscription, Task};

use paperloom_common::widgets::{self, Widgets, BUILT_IN, MAX_PLACED};

mod stacks;

use super::widgets_looks::{self, Previews, CARD};
use super::widgets_online::{self, Catalog};
use crate::theme;
use crate::ui;

#[derive(Debug, Clone)]
pub enum Message {
    SetEnabled(bool),
    Add(&'static str),
    Remove(u32),
    Select(u32),
    /// Dragged on the Layout page: moved, not saved yet.
    Moved(u32, f32, f32),
    /// Let go of a drag: save.
    Dropped,
    Scale(u32, f32),
    Opacity(u32, f32),
    Drag(u32, bool),
    Menu(u32, bool),
    /// Go to Browse (the looks gallery).
    Browse,
    /// Go to Add widgets.
    AddWidgets,
    Tick,
    PreviewsReady(widgets_looks::Drawn),
    /// The clock and date previews, redrawn for a new minute.
    TimesReady(widgets_looks::Drawn),
    /// Show only the looks for this kind (empty = all).
    Filter(String),
    /// Use this look for the selected widget.
    UseLook(String),
    /// Put a new widget of this kind on the desktop with this look.
    AddWithLook(String, String),
    SetLook(u32, String),
    /// Open a widget's own page.
    Open(u32),
    /// Back from a widget's page to My widgets.
    Back,
    /// Take every widget of this kind off the desktop.
    RemoveKind(&'static str),
    /// Show (or hide) the widgets of this kind on Add a widget.
    Expand(&'static str),
    /// A slider was let go: save.
    Commit,
    /// Go to the Stacks page.
    ShowStacks,
    /// Put a stack preset on the desktop (by its id).
    AddStack(String),
    RemoveStack(u32),
    OpenStack(u32),
    /// Keep a stack's widgets, as separate ones.
    Ungroup(u32),
    StackScale(u32, f32),
    StacksTab(BrowseTab),
    StackOnline(widgets_online::Message),
    /// Delete an installed stack preset (its folder).
    RemovePreset(String),
    PresetRemoved(Result<(), String>),
    /// Put back what was just removed.
    Undo,
    /// A widget's ⋯ menu: open it, close it.
    ShowMenu(u32),
    CloseMenu,
    /// Close the line saying what just happened.
    Dismiss,
    /// The online library, showing looks for this kind.
    MoreLooks(String),
    /// Delete an installed or imported look (its folder name).
    RemoveLook(String),
    LookRemoved(Result<(), String>),
    /// Bring in Rainmeter skins: a package or file (false) or a folder (true).
    ImportRainmeter(bool),
    /// Looks added, parts left out — or why nothing could be.
    Imported(Imported),
    /// Browse: Installed or Online.
    Tab(BrowseTab),
    Online(widgets_online::Message),
}

/// A Rainmeter import: the looks added and the parts left out, or why nothing was.
pub type Imported = Result<(Vec<String>, Vec<String>), String>;

/// A line at the top of the page: what just happened, and the next step.
#[derive(Debug, Clone)]
pub struct Notice {
    text: String,
    action: Option<(String, Message)>,
}

impl Notice {
    fn new(text: impl Into<String>, action: Option<(&str, Message)>) -> Option<Self> {
        Some(Self { text: text.into(), action: action.map(|(label, message)| (label.to_string(), message)) })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowseTab {
    Installed,
    Online,
}

pub struct State {
    widgets: Widgets,
    selected: Option<u32>,
    running: bool,
    /// Why the widgets process couldn't be started, if it couldn't.
    problem: Option<String>,
    /// The primary screen's size, which the Layout page draws to scale.
    screen: (u32, u32),
    previews: Previews,
    previews_loading: bool,
    filter: String,
    tab: BrowseTab,
    online: widgets_online::State,
    /// The widget whose own page is open, on My widgets.
    open: Option<u32>,
    /// The kind whose placed widgets are listed on Add a widget.
    expanded: Option<&'static str>,
    importing: bool,
    imported: Option<Imported>,
    /// What just happened, with the obvious next step.
    notice: Option<Notice>,
    /// What the last removal took off the desktop (for Undo).
    removed: Vec<widgets::Placed>,
    /// The widget whose ⋯ menu is open.
    menu: Option<u32>,
    /// The stack whose own page is open, on My widgets.
    open_stack: Option<u32>,
    stacks_tab: BrowseTab,
    stacks_online: widgets_online::State,
    /// The stacks a removal took off (for Undo), with their widgets in `removed`.
    removed_stacks: Vec<paperloom_common::stacks::Stack>,
    /// The minute the clock and date previews show.
    minute: u64,
    /// Which wallpaper the previews were drawn on.
    wallpaper: Option<std::path::PathBuf>,
}

/// Which sub-page the widgets body shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Mine,
    /// Ready-made sets of widgets.
    Stacks,
    /// The looks gallery.
    Browse,
    /// Every kind of widget that can be added.
    Add,
    Layout,
}

impl State {
    pub fn new() -> Self {
        let widgets = Widgets::load();
        let selected = widgets.placed.first().map(|placed| placed.id);
        Self { widgets, selected, running: widgets::is_running(), problem: None, screen: paperloom_common::primary_size().unwrap_or((1920, 1080)), previews: Previews::new(), previews_loading: false, filter: String::new(), tab: BrowseTab::Installed, online: widgets_online::State::new(Catalog::Looks), open: None, expanded: None, importing: false, imported: None, notice: None, removed: Vec::new(), menu: None, open_stack: None, stacks_tab: BrowseTab::Installed, stacks_online: widgets_online::State::new(Catalog::Stacks), removed_stacks: Vec::new(), minute: 0, wallpaper: paperloom_common::EngineState::load().current_wallpaper }
    }

    /// Opens the Browse page on its Online tab (`--online`).
    pub fn show_online(&mut self) {
        self.tab = BrowseTab::Online;
    }

    /// Back to the list of widgets (My widgets picked again in the sidebar).
    pub fn close(&mut self) {
        self.open = None;
    }

    /// Re-reads the file (another window, the tray, or a reset may have changed it).
    pub fn open(&mut self) -> Task<Message> {
        // A way back stays until it is dismissed; other lines are about the page you left.
        if !self.notice.as_ref().is_some_and(|notice| matches!(notice.action, Some((_, Message::Undo)))) {
            self.notice = None;
        }
        self.widgets = Widgets::load();
        self.running = widgets::is_running();
        if self.selected.is_none_or(|id| !self.widgets.placed.iter().any(|placed| placed.id == id)) {
            self.selected = self.widgets.placed.first().map(|placed| placed.id);
        }
        if self.open.is_some_and(|id| !self.widgets.placed.iter().any(|placed| placed.id == id)) {
            self.open = None;
        }
        // The gallery's previews are drawn once, on a thread of their own.
        if self.previews.is_empty() && !self.previews_loading {
            self.previews_loading = true;
            return widgets_looks::load(Message::PreviewsReady);
        }
        Task::none()
    }

    pub fn subscription(&self, active: bool) -> Subscription<Message> {
        if active {
            iced::time::every(Duration::from_secs(2)).map(|_| Message::Tick)
        } else {
            Subscription::none()
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.problem = widgets::set_enabled(enabled).err().map(|err| format!("Couldn't change the widgets: {err}"));
        self.widgets = Widgets::load();
        self.running = widgets::is_running();
        if self.selected.is_none() {
            self.selected = self.widgets.placed.first().map(|placed| placed.id);
        }
    }

    /// Saves, and makes sure the process is running when widgets are on.
    fn apply(&mut self) {
        self.problem = None;
        if let Err(err) = self.widgets.save() {
            self.problem = Some(format!("Couldn't save the widget settings: {err}"));
            return;
        }
        if self.widgets.enabled {
            if let Err(err) = widgets::start() {
                self.problem = Some(format!("Couldn't start the widgets: {err}"));
            }
        }
        self.running = widgets::is_running();
    }

    /// Every couple of seconds while a widgets page is open: the process state,
    /// the library retrying when it was offline, and previews that went stale
    /// (a new wallpaper; a new minute for the clocks).
    fn tick(&mut self) -> Task<Message> {
        self.running = widgets::is_running();
        let mut tasks = Vec::new();
        if self.stacks_tab == BrowseTab::Online {
            tasks.push(self.stacks_online.retry().map(Message::StackOnline));
        }
        if self.tab == BrowseTab::Online {
            tasks.push(self.online.retry().map(Message::Online));
        }
        if self.previews_loading {
            return Task::batch(tasks);
        }
        if self.previews.is_empty() {
            // Opened straight from the command line: nothing asked for the previews yet.
            if self.tab == BrowseTab::Online {
                tasks.push(self.online.opened().map(Message::Online));
            }
            if self.stacks_tab == BrowseTab::Online {
                tasks.push(self.stacks_online.opened().map(Message::StackOnline));
            }
            self.previews_loading = true;
            tasks.push(widgets_looks::load(Message::PreviewsReady));
            return Task::batch(tasks);
        }
        // A new wallpaper: draw everything on it. A new minute: the clocks.
        let wallpaper = paperloom_common::EngineState::load().current_wallpaper;
        if wallpaper != self.wallpaper {
            self.wallpaper = wallpaper;
            self.previews_loading = true;
            tasks.push(widgets_looks::load(Message::PreviewsReady));
        } else {
            let minute = paperloom_common::log::now_unix() / 60;
            if minute != self.minute {
                self.minute = minute;
                tasks.push(widgets_looks::load_times(Message::TimesReady));
            }
        }
        Task::batch(tasks)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick => return self.tick(),
            Message::Browse | Message::AddWidgets | Message::ShowStacks => return Task::none(),
            Message::TimesReady(drawn) => {
                self.previews.merge(drawn);
                return Task::none();
            }
            Message::PreviewsReady(drawn) => {
                self.minute = paperloom_common::log::now_unix() / 60;
                self.previews = widgets_looks::into_previews(drawn);
                self.previews_loading = false;
                return Task::none();
            }
            Message::Tab(tab) => {
                self.tab = tab;
                return if tab == BrowseTab::Online { self.online.opened().map(Message::Online) } else { Task::none() };
            }
            Message::Online(widgets_online::Message::Use(skin)) => return self.update(Message::UseLook(skin)),
            Message::Online(widgets_online::Message::Add(kind, skin)) => return self.update(Message::AddWithLook(kind, skin)),
            Message::Online(message) => {
                let installed = match &message {
                    widgets_online::Message::Done(id, Ok(())) => Some(id.clone()),
                    _ => None,
                };
                let (task, changed) = self.online.update(message);
                let mut tasks = vec![task.map(Message::Online)];
                // Installed: offer to use it right here, not somewhere else.
                if let Some(skin) = installed.and_then(|id| paperloom_skin::get(&id)) {
                    self.notice = match self.target_for(&skin.kinds) {
                        Some(target) => Notice::new(format!("{} is installed.", skin.name), Some((&format!("Use it on {}", self.title_of(target)), Message::UseLook(skin.id.clone())))),
                        None => Notice::new(format!("{} is installed.", skin.name), Some(("Add it to the desktop", Message::AddWithLook(skin.kinds.first().cloned().unwrap_or_default(), skin.id.clone())))),
                    };
                }
                if changed {
                    // A look was installed or removed: draw the previews again
                    // (the old ones stay up until the new ones are ready).
                    self.previews_loading = true;
                    tasks.push(widgets_looks::load(Message::PreviewsReady));
                    // A widget whose look is gone falls back to its default.
                    self.widgets = Widgets::load();
                }
                return Task::batch(tasks);
            }
            Message::Filter(kind) => {
                self.filter = kind;
                return Task::none();
            }
            Message::UseLook(skin) => {
                let Some(look) = paperloom_skin::get(&skin) else { return Task::none() };
                let Some(id) = self.target_for(&look.kinds).map(|target| target.id) else { return Task::none() };
                let title = self.widgets.placed.iter().find(|placed| placed.id == id).map(|placed| self.title_of(placed)).unwrap_or_default();
                self.notice = Notice::new(format!("Your {title} now uses {}.", look.name), Some(("Open it", Message::Open(id))));
                if let Some(target) = self.widgets.placed.iter_mut().find(|target| target.id == id) {
                    target.skin = skin;
                }
            }
            Message::AddStack(id) => {
                self.widgets = Widgets::load();
                let Some(preset) = paperloom_common::stacks::preset(&id) else { return Task::none() };
                match self.widgets.add_stack(&preset, self.screen) {
                    Some(stack) => {
                        self.widgets.enabled = true;
                        self.selected = self.widgets.members(stack).next().map(|placed| placed.id);
                        self.notice = Notice::new(format!("{} is on your desktop. Drag any part of it to move the whole stack.", preset.name), Some(("Open it", Message::OpenStack(stack))));
                    }
                    None => {
                        self.problem = Some(format!("{} has {} widgets and you can place up to {MAX_PLACED}: remove some first.", preset.name, preset.widget.len()));
                        return Task::none();
                    }
                }
            }
            Message::RemoveStack(id) => {
                self.menu = None;
                if let Some(stack) = self.widgets.stack(id).cloned() {
                    self.removed = self.widgets.members(id).cloned().collect();
                    self.removed_stacks = vec![stack.clone()];
                    self.notice = Notice::new(format!("{} removed from your desktop.", stack.name), Some(("Undo", Message::Undo)));
                }
                self.widgets.remove_stack(id);
                if self.open_stack == Some(id) {
                    self.open_stack = None;
                }
                if self.selected.is_some_and(|id| !self.widgets.placed.iter().any(|placed| placed.id == id)) {
                    self.selected = self.widgets.placed.first().map(|placed| placed.id);
                }
            }
            Message::OpenStack(id) => {
                self.menu = None;
                self.open = None;
                self.open_stack = Some(id);
                self.selected = self.widgets.members(id).next().map(|placed| placed.id).or(self.selected);
                return Task::none();
            }
            Message::Ungroup(id) => {
                let name = self.widgets.stack(id).map(|stack| stack.name.clone()).unwrap_or_default();
                self.widgets.ungroup(id);
                self.open_stack = None;
                self.notice = Notice::new(format!("{name} is now separate widgets. Each one moves on its own."), None);
            }
            Message::StackScale(id, value) => {
                self.widgets.scale_stack(id, value, self.screen);
                return Task::none();
            }
            Message::StacksTab(tab) => {
                self.stacks_tab = tab;
                return if tab == BrowseTab::Online { self.stacks_online.opened().map(Message::StackOnline) } else { Task::none() };
            }
            Message::StackOnline(widgets_online::Message::AddStack(id)) => return self.update(Message::AddStack(id)),
            Message::StackOnline(message) => {
                let installed = match &message {
                    widgets_online::Message::Done(id, Ok(())) => Some(id.clone()),
                    _ => None,
                };
                let (task, changed) = self.stacks_online.update(message);
                let mut tasks = vec![task.map(Message::StackOnline)];
                if let Some(preset) = installed.and_then(|id| paperloom_common::stacks::preset(&id)) {
                    self.notice = Notice::new(format!("{} is installed.", preset.name), Some(("Add to desktop", Message::AddStack(preset.id.clone()))));
                }
                if changed {
                    paperloom_skin::refresh();
                    self.previews_loading = true;
                    tasks.push(widgets_looks::load(Message::PreviewsReady));
                    self.widgets = Widgets::load();
                }
                return Task::batch(tasks);
            }
            Message::RemovePreset(folder) => {
                return widgets_looks::blocking(move || paperloom_fetch::looks::remove_stack(&folder, &paperloom_common::stacks_dir()), Message::PresetRemoved);
            }
            Message::PresetRemoved(result) => {
                self.problem = result.err().map(|err| format!("Couldn't remove that stack: {err}"));
                self.previews_loading = true;
                return widgets_looks::load(Message::PreviewsReady);
            }
            Message::Undo => {
                for stack in std::mem::take(&mut self.removed_stacks) {
                    if !self.widgets.stacks.iter().any(|other| other.id == stack.id) {
                        self.widgets.stacks.push(stack);
                    }
                }
                for placed in std::mem::take(&mut self.removed) {
                    if self.widgets.placed.len() < MAX_PLACED && !self.widgets.placed.iter().any(|other| other.id == placed.id) {
                        self.widgets.placed.push(placed);
                    }
                }
                self.notice = None;
            }
            Message::ShowMenu(id) => {
                self.menu = Some(id);
                self.selected = Some(id);
                return Task::none();
            }
            Message::CloseMenu => {
                self.menu = None;
                return Task::none();
            }
            Message::Dismiss => {
                self.notice = None;
                return Task::none();
            }
            Message::MoreLooks(kind) => {
                self.menu = None;
                self.tab = BrowseTab::Online;
                self.filter = kind;
                return Task::batch([self.online.opened().map(Message::Online), Task::done(Message::Browse)]);
            }
            Message::RemoveLook(folder) => {
                return widgets_looks::blocking(move || paperloom_fetch::looks::remove(&folder, &paperloom_common::skins_dir()), Message::LookRemoved);
            }
            Message::LookRemoved(result) => {
                paperloom_skin::refresh();
                self.problem = result.err().map(|err| format!("Couldn't remove that look: {err}"));
                // Widgets that used it are back on their kind's default look.
                self.widgets = Widgets::load();
                self.previews_loading = true;
                return widgets_looks::load(Message::PreviewsReady);
            }
            Message::SetLook(id, skin) => {
                if let Some(target) = self.widgets.placed.iter_mut().find(|target| target.id == id) {
                    target.skin = skin;
                }
            }
            Message::AddWithLook(kind, skin) => {
                self.widgets = Widgets::load();
                match self.widgets.add(&kind) {
                    Some(id) => {
                        if let Some(target) = self.widgets.placed.iter_mut().find(|target| target.id == id) {
                            target.skin = skin.clone();
                        }
                        self.selected = Some(id);
                        self.widgets.enabled = true;
                        let name = widgets::entry(&kind).map_or(kind.as_str(), |entry| entry.name);
                        let look = paperloom_skin::get(&skin).map_or(String::new(), |look| format!(" with the {} look", look.name));
                        self.notice = Notice::new(format!("{name}{look} is on your desktop."), Some(("Open it", Message::Open(id))));
                    }
                    None => {
                        self.problem = Some(format!("You can place up to {MAX_PLACED} widgets."));
                        return Task::none();
                    }
                }
            }
            Message::SetEnabled(enabled) => {
                self.set_enabled(enabled);
                return Task::none();
            }
            Message::Add(kind) => {
                self.widgets = Widgets::load();
                if let Some(id) = self.widgets.add(kind) {
                    self.selected = Some(id);
                    self.expanded = Some(kind);
                    self.widgets.enabled = true;
                    let name = widgets::entry(kind).map_or(kind, |entry| entry.name);
                    self.notice = Notice::new(format!("{name} is on your desktop."), Some(("Change its look", Message::Open(id))));
                } else {
                    self.problem = Some(format!("You can place up to {MAX_PLACED} widgets."));
                    return Task::none();
                }
            }
            Message::Remove(id) => {
                self.menu = None;
                if let Some(placed) = self.widgets.placed.iter().find(|placed| placed.id == id) {
                    self.notice = Notice::new(format!("{} removed from your desktop.", self.title_of(placed)), Some(("Undo", Message::Undo)));
                    self.removed = vec![placed.clone()];
                    self.removed_stacks = self.widgets.stack(placed.stack).cloned().into_iter().collect();
                }
                self.widgets.remove(id);
                self.widgets.tidy_stacks();
                if self.open == Some(id) {
                    self.open = None;
                }
                if self.selected == Some(id) {
                    self.selected = self.widgets.placed.first().map(|placed| placed.id);
                }
            }
            Message::Select(id) => {
                self.selected = Some(id);
                return Task::none();
            }
            Message::Open(id) => {
                self.menu = None;
                self.open_stack = None;
                self.selected = Some(id);
                self.open = Some(id);
                return Task::none();
            }
            Message::Back => {
                // From a widget in a stack, back to that stack.
                if let Some(stack) = self.open.and_then(|id| self.widgets.placed.iter().find(|placed| placed.id == id)).map(|placed| placed.stack).filter(|stack| *stack != 0) {
                    self.open_stack = Some(stack);
                } else {
                    self.open_stack = None;
                }
                self.open = None;
                return Task::none();
            }
            Message::ImportRainmeter(folder) => {
                let dialog = rfd::FileDialog::new().set_title("Import a Rainmeter skin");
                // Rainmeter keeps its skins in Documents\Rainmeter\Skins.
                let skins = std::env::var_os("USERPROFILE").map(|home| std::path::PathBuf::from(home).join("Documents").join("Rainmeter").join("Skins")).filter(|path| path.is_dir());
                let dialog = match skins {
                    Some(skins) => dialog.set_directory(skins),
                    None => dialog,
                };
                let picked = if folder { dialog.pick_folder() } else { dialog.add_filter("Rainmeter skin", &["rmskin", "zip", "ini"]).pick_file() };
                let Some(path) = picked else { return Task::none() };
                self.importing = true;
                self.imported = None;
                return widgets_looks::blocking(
                    move || paperloom_skin::rainmeter::import(&path, &paperloom_common::skins_dir()).map(|report| (report.looks, report.left_out)),
                    Message::Imported,
                );
            }
            Message::Imported(result) => {
                self.importing = false;
                if result.is_ok() {
                    paperloom_skin::refresh();
                    self.filter.clear();
                    self.previews_loading = true;
                    self.imported = Some(result);
                    return widgets_looks::load(Message::PreviewsReady);
                }
                self.imported = Some(result);
                return Task::none();
            }
            Message::Expand(kind) => {
                self.expanded = if self.expanded == Some(kind) { None } else { Some(kind) };
                return Task::none();
            }
            Message::RemoveKind(kind) => {
                self.removed = self.widgets.placed.iter().filter(|placed| placed.kind == kind).cloned().collect();
                let name = widgets::entry(kind).map_or(kind, |entry| entry.name);
                let what = if self.removed.len() == 1 { format!("{name} removed") } else { format!("All {} {name} widgets removed", self.removed.len()) };
                self.notice = Notice::new(format!("{what} from your desktop."), Some(("Undo", Message::Undo)));
                let ids: Vec<u32> = self.removed.iter().map(|placed| placed.id).collect();
                for id in ids {
                    self.widgets.remove(id);
                }
                self.widgets.tidy_stacks();
                if self.selected.is_some_and(|id| !self.widgets.placed.iter().any(|placed| placed.id == id)) {
                    self.selected = self.widgets.placed.first().map(|placed| placed.id);
                }
                self.open = self.open.filter(|id| self.widgets.placed.iter().any(|placed| placed.id == *id));
            }
            Message::Moved(id, x, y) => {
                // A widget in a stack takes its whole stack along.
                self.widgets.move_member(id, x, y, self.screen);
                return Task::none();
            }
            Message::Dropped => {}
            Message::Scale(id, value) | Message::Opacity(id, value) => {
                if let Some(target) = self.widgets.placed.iter_mut().find(|target| target.id == id) {
                    match message {
                        Message::Scale(..) => target.scale = value.clamp(0.5, 3.0),
                        _ => target.opacity = value.clamp(0.1, 1.0),
                    }
                }
                // Saved when the slider is let go, not on every step of the drag.
                return Task::none();
            }
            Message::Commit => {}
            Message::Drag(id, value) => {
                if let Some(target) = self.widgets.placed.iter_mut().find(|target| target.id == id) {
                    target.drag = value;
                }
            }
            Message::Menu(id, value) => {
                if let Some(target) = self.widgets.placed.iter_mut().find(|target| target.id == id) {
                    target.menu = value;
                }
            }
        }
        self.apply();
        Task::none()
    }

    pub fn header<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let state = if self.running { "Showing on the desktop" } else if self.widgets.enabled { "Starting…" } else { "Hidden" };
        row![
            text(state).size(13).color(if self.running { t.success } else { t.muted_foreground }),
            toggler(self.widgets.enabled).label("Show widgets").on_toggle(Message::SetEnabled),
        ]
        .spacing(16)
        .align_y(Alignment::Center)
        .into()
    }

    pub fn body<'a>(&'a self, mode: Mode, t: theme::Tokens) -> Element<'a, Message> {
        let mut page = column![].spacing(16);
        if let Some(problem) = &self.problem {
            page = page.push(text(problem.as_str()).size(13).color(t.destructive));
        }
        if let Some(notice) = &self.notice {
            let mut line = row![text(notice.text.as_str()).size(14).color(t.foreground).width(Length::Fill)].spacing(10).align_y(Alignment::Center);
            if let Some((label, message)) = &notice.action {
                line = line.push(button(ui::label(label.as_str()).color(ui::on_accent())).style(ui::primary).on_press(message.clone()));
            }
            line = line.push(button(ui::label("✕")).style(ui::ghost).on_press(Message::Dismiss));
            page = page.push(
                container(line)
                    .padding([10, 16])
                    .width(Length::Fill)
                    .style(move |_theme| container::Style { background: Some(Background::Color(t.surface)), border: Border { color: t.primary, width: 1.0, radius: 8.0.into() }, ..container::Style::default() }),
            );
        }
        // Placed but switched off: say so where it can't be missed, with the way back.
        if !self.widgets.enabled && !self.widgets.placed.is_empty() {
            page = page.push(panel(
                row![
                    column![
                        text("Your widgets are hidden").size(15).color(t.foreground),
                        text("They keep their places and looks. Show them to put them back on the desktop.").size(13).color(t.muted_foreground),
                    ]
                    .spacing(2),
                    Space::new().width(Length::Fill),
                    button(ui::label("Show widgets").color(ui::on_accent())).style(ui::primary).on_press(Message::SetEnabled(true)),
                ]
                .spacing(12)
                .align_y(Alignment::Center)
                .into(),
                t,
            ));
        }
        page = page.push(match mode {
            Mode::Browse => self.looks(t),
            Mode::Add => self.types(t),
            Mode::Stacks => self.stacks_page(t),
            Mode::Mine => match self.open_stack.and_then(|id| self.widgets.stack(id)).filter(|_| self.open.is_none()) {
                Some(stack) => self.stack_details(stack, t),
                None => match self.open.and_then(|id| self.widgets.placed.iter().find(|placed| placed.id == id)) {
                    Some(placed) => self.details(placed, t),
                    None => self.mine(t),
                },
            },
            Mode::Layout => self.layout(t),
        });
        ui::scroll_page(page).height(Length::Fill).into()
    }

    /// The name a placed widget goes by: its kind, numbered when there are several.
    fn title_of(&self, placed: &widgets::Placed) -> String {
        let name = widgets::entry(&placed.kind).map_or(placed.kind.as_str(), |entry| entry.name);
        let same: Vec<u32> = self.widgets.placed.iter().filter(|other| other.kind == placed.kind).map(|other| other.id).collect();
        match same.iter().position(|id| *id == placed.id) {
            Some(index) if same.len() > 1 => format!("{name} {}", index + 1),
            _ => name.to_string(),
        }
    }

    /// A placed widget's card: click it for its page; Remove takes only this
    /// one off; ⋯ has its looks and everything else, without leaving the page.
    fn placed_card<'a>(&'a self, placed: &'a widgets::Placed, t: theme::Tokens) -> Element<'a, Message> {
        let look = widgets_looks::skin_of(placed).map_or("", |skin| skin.name.as_str());
        let detail = format!("{look} look · {}", where_on_screen(placed.x, placed.y));
        let actions: Vec<Element<'a, Message>> = vec![
            button(ui::label("Remove")).style(ui::secondary).on_press(Message::Remove(placed.id)).into(),
            button(iced::widget::svg(crate::icons::ellipsis()).width(16).height(16).style(move |_theme, _status| iced::widget::svg::Style { color: Some(t.foreground) }))
                .padding([6, 10])
                .style(ui::ghost)
                .on_press(Message::ShowMenu(placed.id))
                .into(),
        ];
        card(self.previews.of(placed), self.title_of(placed), detail, actions, Some(Message::Open(placed.id)), false, t)
    }

    /// A widget's ⋯ menu, over the whole window: its looks, and what else can be done to it.
    pub fn overlay<'a>(&'a self, t: theme::Tokens) -> Option<Element<'a, Message>> {
        let placed = self.menu.and_then(|id| self.widgets.placed.iter().find(|placed| placed.id == id))?;
        let id = placed.id;
        let current = widgets_looks::skin_of(placed).map(|skin| skin.id.clone());
        let mut looks: Vec<Element<'a, Message>> = paperloom_skin::for_kind(&placed.kind)
            .into_iter()
            .map(|skin| {
                let chosen = current.as_deref() == Some(skin.id.as_str());
                let detail = if chosen { "In use".to_string() } else { format!("by {}", skin.author) };
                card(self.previews.card(&skin.id, &placed.kind), skin.name.clone(), detail, Vec::new(), (!chosen).then(|| Message::SetLook(id, skin.id.clone())), chosen, t)
            })
            .collect();
        looks.push(more_looks_card(placed.kind.clone(), t));
        let body = column![
            text("Look").size(16).color(t.foreground),
            text("Click one to use it. The desktop changes straight away.").size(13).color(t.muted_foreground),
            row(looks).spacing(16).wrap().vertical_spacing(16),
            Space::new().height(Length::Fixed(8.0)),
            row![
                ui::tile("Size, place and behaviour", "Open this widget's own page", Message::Open(id), false, t),
                ui::tile("Remove from the desktop", "Only this one; you can undo it", Message::Remove(id), true, t),
            ]
            .spacing(12),
        ]
        .spacing(12);
        let look = widgets_looks::skin_of(placed).map_or(String::new(), |skin| format!("{} look", skin.name));
        Some(ui::screen(self.title_of(placed), Some(format!("{look} · {}", where_on_screen(placed.x, placed.y))), ui::scroll_page(body).into(), Message::CloseMenu, t))
    }

    fn layout<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        if self.widgets.placed.is_empty() {
            return self.mine(t);
        }
        column![
            text("This is your screen. Drag a widget to move it — it snaps to the edges and the middle, and is saved when you let go.").size(13).color(t.muted_foreground),
            super::widgets_layout::view(&self.widgets.placed, self.selected, self.screen, &self.previews, 560.0, t),
        ]
        .spacing(12)
        .into()
    }

    fn mine<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        if self.widgets.placed.is_empty() {
            // Show what they would get, and the one click that gets it.
            let samples: Vec<Element<'a, Message>> = ["clock", "cpu", "date"]
                .into_iter()
                .filter_map(|kind| {
                    let entry = widgets::entry(kind)?;
                    let skin = paperloom_skin::default_for(kind)?;
                    Some(card(self.previews.card(&skin.id, kind), entry.name.to_string(), entry.about.to_string(), vec![add_button(entry.kind)], None, false, t))
                })
                .collect();
            return column![
                column![
                    text("Put a clock or a meter on your desktop").size(20).color(t.foreground),
                    text("Widgets sit on top of your wallpaper — this is how they look on yours. Add one, then click it to change its look, size and place.").size(13).color(t.muted_foreground),
                ]
                .spacing(4),
                row(samples).spacing(16).wrap().vertical_spacing(16),
                row![
                    button(ui::label("Pick a ready-made stack").color(ui::on_accent())).style(ui::primary).on_press(Message::ShowStacks),
                    button(ui::label("See every widget")).style(ui::ghost).on_press(Message::AddWidgets),
                ]
                .spacing(8),
            ]
            .spacing(16)
            .into();
        }

        let mut cards: Vec<Element<'a, Message>> = self.widgets.stacks.iter().map(|stack| self.stack_card(stack, t)).collect();
        cards.extend(self.widgets.placed.iter().filter(|placed| placed.stack == 0).map(|placed| self.placed_card(placed, t)));
        if self.widgets.placed.len() < MAX_PLACED {
            cards.push(add_card(t));
        }
        column![
            text("Click a widget to change its look, size and place.").size(13).color(t.muted_foreground),
            row(cards).spacing(16).wrap().vertical_spacing(16),
        ]
        .spacing(12)
        .into()
    }

    /// A widget's own page: where it is, its look, its size, how it behaves.
    fn details<'a>(&'a self, placed: &'a widgets::Placed, t: theme::Tokens) -> Element<'a, Message> {
        let id = placed.id;
        let heading = row![
            button(ui::label("← My widgets")).style(ui::ghost).on_press(Message::Back),
            text(self.title_of(placed)).size(20).color(t.foreground),
            Space::new().width(Length::Fill),
            button(ui::label("Remove from desktop")).style(ui::danger).on_press(Message::Remove(id)),
        ]
        .spacing(12)
        .align_y(Alignment::Center);

        let screen = section(
            "Where it sits",
            "Your screen, with this widget outlined. Drag it to move it.",
            super::widgets_layout::view(&self.widgets.placed, Some(id), self.screen, &self.previews, 380.0, t),
            t,
        );

        let current = widgets_looks::skin_of(placed).map(|skin| skin.id.clone());
        let mut looks: Vec<Element<'a, Message>> = paperloom_skin::for_kind(&placed.kind)
            .into_iter()
            .map(|skin| {
                let chosen = current.as_deref() == Some(skin.id.as_str());
                let detail = if chosen { "In use".to_string() } else { format!("by {}", skin.author) };
                card(self.previews.card(&skin.id, &placed.kind), skin.name.clone(), detail, Vec::new(), (!chosen).then(|| Message::SetLook(id, skin.id.clone())), chosen, t)
            })
            .collect();
        looks.push(more_looks_card(placed.kind.clone(), t));
        let looks = section("Look", "Click a look to use it. What the widget shows stays the same.", row(looks).spacing(16).wrap().vertical_spacing(16).into(), t);

        let percent = |value: f32| text(format!("{:.0}%", value * 100.0)).size(13).color(t.muted_foreground).width(Length::Fixed(48.0));
        let line = |label: &'static str, control: Element<'a, Message>, value: Element<'a, Message>| -> Element<'a, Message> {
            row![text(label).size(13).color(t.foreground).width(Length::Fixed(110.0)), control, value].spacing(12).align_y(Alignment::Center).into()
        };
        let size = section(
            "Size",
            "",
            column![
                line("Size", slider(0.5..=2.0, placed.scale, move |value| Message::Scale(id, value)).step(0.05_f32).on_release(Message::Commit).into(), percent(placed.scale).into()),
                line("Opacity", slider(0.1..=1.0, placed.opacity, move |value| Message::Opacity(id, value)).step(0.05_f32).on_release(Message::Commit).into(), percent(placed.opacity).into()),
            ]
            .spacing(10)
            .into(),
            t,
        );

        let behaviour = section(
            "On the desktop",
            if placed.interactive() {
                "It sits above your desktop icons so you can grab it. Turn both off and it sits below them, and every click goes through to the desktop."
            } else {
                "Clicks go through it to the desktop, and it sits below your desktop icons. Move it here or in Arrange."
            },
            column![
                toggler(placed.drag).label("Move it by dragging on the desktop").on_toggle(move |value| Message::Drag(id, value)),
                toggler(placed.menu).label("Right-click it for a menu").on_toggle(move |value| Message::Menu(id, value)),
            ]
            .spacing(8)
            .into(),
            t,
        );

        column![heading, screen, looks, size, behaviour].spacing(16).into()
    }

    /// Every kind of widget that can be added, with the ones already placed folded under it.
    fn types<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let mut list = column![text("Pick what you want on your desktop. You can change how it looks afterwards.").size(13).color(t.muted_foreground)].spacing(12);
        for entry in BUILT_IN {
            let placed: Vec<&widgets::Placed> = self.widgets.placed.iter().filter(|placed| placed.kind == entry.kind).collect();
            let preview = paperloom_skin::default_for(entry.kind).and_then(|skin| self.previews.card(&skin.id, entry.kind));
            let mut actions = row![add_button(entry.kind)].spacing(8).align_y(Alignment::Center);
            if !placed.is_empty() {
                let open = self.expanded == Some(entry.kind);
                let count = if placed.len() == 1 { "1 on your desktop".to_string() } else { format!("{} on your desktop", placed.len()) };
                actions = actions.push(button(ui::label(format!("{count} {}", if open { "▴" } else { "▾" }))).style(ui::secondary).on_press(Message::Expand(entry.kind)));
                actions = actions.push(button(ui::label(if placed.len() == 1 { "Remove it" } else { "Remove all" })).style(ui::ghost).on_press(Message::RemoveKind(entry.kind)));
            }
            let mut body = column![
                row![
                    picture(preview, t),
                    column![text(entry.name).size(18).color(t.foreground), text(entry.about).size(13).color(t.muted_foreground), Space::new().height(Length::Fixed(8.0)), actions].spacing(4),
                ]
                .spacing(16)
                .align_y(Alignment::Center),
            ]
            .spacing(14);
            if self.expanded == Some(entry.kind) && !placed.is_empty() {
                body = body.push(row(placed.into_iter().map(|placed| self.placed_card(placed, t))).spacing(16).wrap().vertical_spacing(16));
            }
            list = list.push(panel(body.into(), t));
        }
        list.into()
    }
}

/// Words for where a widget sits, from its position as fractions of the screen.
fn where_on_screen(x: f32, y: f32) -> String {
    let across = if x < 0.33 { "left" } else if x < 0.6 { "middle" } else { "right" };
    let down = if y < 0.33 { "top" } else if y < 0.6 { "middle" } else { "bottom" };
    match (down, across) {
        ("middle", "middle") => "middle of the screen".to_string(),
        (down, across) => format!("{down} {across}"),
    }
}

/// A preview the size of a card, or a quiet placeholder while it is drawn.
fn picture<'a, M: 'a>(handle: Option<&image::Handle>, t: theme::Tokens) -> Element<'a, M> {
    let (w, h) = (CARD.0 as f32, CARD.1 as f32);
    match handle {
        Some(handle) => container(image(handle.clone()).width(Length::Fixed(w)).height(Length::Fixed(h)))
            .style(|_theme| container::Style { border: Border { radius: 6.0.into(), ..Border::default() }, ..container::Style::default() })
            .into(),
        None => container(text("Drawing…").size(12).color(t.muted_foreground))
            .center_x(Length::Fixed(w))
            .center_y(Length::Fixed(h))
            .style(move |_theme| container::Style { background: Some(Background::Color(t.muted)), border: Border { radius: 6.0.into(), ..Border::default() }, ..container::Style::default() })
            .into(),
    }
}

/// The one card used everywhere: picture, name, a line of detail, and
/// optional buttons. Always the same size. Clickable when `on_press` is set.
pub(super) fn card<'a, M: Clone + 'a>(handle: Option<&image::Handle>, name: String, detail: String, actions: Vec<Element<'a, M>>, on_press: Option<M>, chosen: bool, t: theme::Tokens) -> Element<'a, M> {
    let mut content = column![
        picture(handle, t),
        text(name).size(15).color(t.foreground),
        text(detail).size(12).color(if chosen { t.success } else { t.muted_foreground }),
    ]
    .spacing(4);
    let has_actions = !actions.is_empty();
    if has_actions {
        content = content.push(Space::new().height(Length::Fixed(4.0))).push(row(actions).spacing(8));
    }
    // Cards of one kind are all as tall, so a row of them lines up.
    let height = CARD.1 as f32 + if has_actions { 104.0 } else { 64.0 };
    let border = if chosen { t.primary } else { t.border };
    let framed = container(content).padding(10).width(Length::Fixed(CARD.0 as f32 + 22.0)).height(Length::Fixed(height));
    match on_press {
        Some(message) => button(framed)
            .padding(0)
            .style(move |_theme, status| button::Style {
                background: Some(Background::Color(if matches!(status, button::Status::Hovered) { t.muted } else { t.surface })),
                border: Border { color: if matches!(status, button::Status::Hovered) { t.primary } else { border }, width: if chosen { 2.0 } else { 1.0 }, radius: 8.0.into() },
                ..button::Style::default()
            })
            .on_press(message)
            .into(),
        None => framed
            .style(move |_theme| container::Style { background: Some(Background::Color(t.surface)), border: Border { color: border, width: 1.0, radius: 8.0.into() }, ..container::Style::default() })
            .into(),
    }
}

/// A card-sized button with a big plus: the next step after the last widget.
fn add_card<'a>(t: theme::Tokens) -> Element<'a, Message> {
    big_button("+", "Add a widget", "A clock, the date, a meter…", Message::AddWidgets, true, t)
}

fn more_looks_card<'a>(kind: String, t: theme::Tokens) -> Element<'a, Message> {
    big_button("✦", "More looks", "From the online library", Message::MoreLooks(kind), false, t)
}

/// `tall`: as tall as a card with buttons (it sits among them).
fn big_button<'a>(mark: &'static str, name: &'static str, detail: &'static str, message: Message, tall: bool, t: theme::Tokens) -> Element<'a, Message> {
    button(
        container(column![text(mark).size(40).color(t.primary), text(name).size(15).color(t.foreground), text(detail).size(12).color(t.muted_foreground)].spacing(4).align_x(Alignment::Center))
            .center_x(Length::Fixed(CARD.0 as f32 + 22.0))
            .center_y(Length::Fixed(CARD.1 as f32 + if tall { 104.0 } else { 64.0 })),
    )
    .padding(0)
    .style(move |_theme, status| button::Style {
        background: Some(Background::Color(if matches!(status, button::Status::Hovered) { t.muted } else { Color::TRANSPARENT })),
        border: Border { color: if matches!(status, button::Status::Hovered) { t.primary } else { t.border }, width: 1.0, radius: 8.0.into() },
        ..button::Style::default()
    })
    .on_press(message)
    .into()
}

fn add_button<'a>(kind: &'static str) -> Element<'a, Message> {
    button(ui::label("Add to desktop").color(ui::on_accent())).style(ui::primary).on_press(Message::Add(kind)).into()
}

/// A titled block of the widget page.
fn section<'a>(title: &'static str, about: &'static str, content: Element<'a, Message>, t: theme::Tokens) -> Element<'a, Message> {
    let mut block = column![text(title).size(16).color(t.foreground)].spacing(6);
    if !about.is_empty() {
        block = block.push(text(about).size(13).color(t.muted_foreground));
    }
    panel(block.push(Space::new().height(Length::Fixed(4.0))).push(content).into(), t)
}

fn panel<'a>(content: Element<'a, Message>, t: theme::Tokens) -> Element<'a, Message> {
    container(content)
        .padding(16)
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(t.surface)),
            border: Border { color: t.border, width: 1.0, radius: 8.0.into() },
            ..container::Style::default()
        })
        .into()
}

impl State {
    /// Looks: the gallery of looks, each drawn on the user's wallpaper.
    fn looks<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let kinds: Vec<(String, String)> = std::iter::once((String::new(), "All".to_string()))
            .chain(BUILT_IN.iter().filter(|entry| !paperloom_skin::for_kind(entry.kind).is_empty()).map(|entry| (entry.kind.to_string(), entry.name.to_string())))
            .collect();
        let tabs = ui::segmented(
            vec![
                ("On this computer".to_string(), self.tab == BrowseTab::Installed, Message::Tab(BrowseTab::Installed)),
                ("Online library".to_string(), self.tab == BrowseTab::Online, Message::Tab(BrowseTab::Online)),
            ],
            t,
        );
        let filters = ui::segmented(kinds.into_iter().map(|(kind, name)| (name, self.filter == kind, Message::Filter(kind))).collect(), t);
        if self.tab == BrowseTab::Online {
            let target = |kinds: &[String], skin: &str| self.target_for(kinds).map(|placed| (self.title_of(placed), widgets_looks::skin_of(placed).is_some_and(|current| current.id == skin)));
            return column![
                tabs,
                text("More looks, some with their own fonts and images. Install one, then use it on a widget you have or add it as a new one.").size(13).color(t.muted_foreground),
                filters,
                self.online.view(&self.filter, &target, t).map(Message::Online),
            ]
            .spacing(14)
            .into();
        }

        let mut cards = Vec::new();
        for skin in paperloom_skin::all() {
            if !self.filter.is_empty() && !skin.kinds.contains(&self.filter) {
                continue;
            }
            cards.push(self.look_card(skin, t));
        }
        let hint = "Shown on your wallpaper. “Use on…” changes a widget you have; “Add” puts a new one on the desktop.";
        column![tabs, self.import_panel(t), text(hint).size(13).color(t.muted_foreground), filters, row(cards).spacing(16).wrap().vertical_spacing(16)].spacing(14).into()
    }

    /// Bringing in Rainmeter skins, and what came of the last import.
    fn import_panel<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let mut buttons = row![].spacing(8).align_y(Alignment::Center);
        if self.importing {
            buttons = buttons.push(text("Importing…").size(13).color(t.muted_foreground));
        } else {
            buttons = buttons
                .push(button(ui::label("Import .rmskin file…")).style(ui::secondary).on_press(Message::ImportRainmeter(false)))
                .push(button(ui::label("Import a skin folder…")).style(ui::ghost).on_press(Message::ImportRainmeter(true)));
        }
        let mut body = column![
            row![
                column![
                    text("Rainmeter skins").size(15).color(t.foreground),
                    text("Bring in a Rainmeter skin package or folder. Each skin becomes a look here, with its pictures and fonts.").size(13).color(t.muted_foreground),
                ]
                .spacing(2)
                .width(Length::Fill),
                buttons,
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        ]
        .spacing(10);
        match &self.imported {
            Some(Ok((looks, left_out))) => {
                body = body.push(text(format!("Added {}: {}. They're in the gallery below.", if looks.len() == 1 { "1 look".to_string() } else { format!("{} looks", looks.len()) }, looks.join(", "))).size(13).color(t.success));
                if !left_out.is_empty() {
                    let shown: Vec<String> = left_out.iter().take(8).cloned().collect();
                    let more = if left_out.len() > 8 { format!("\n…and {} more", left_out.len() - 8) } else { String::new() };
                    body = body.push(text(format!("Left out, as Paperloom has nothing that matches:\n{}{more}", shown.join("\n"))).size(12).color(t.muted_foreground));
                }
            }
            Some(Err(err)) => body = body.push(text(err.as_str()).size(13).color(t.destructive)),
            None => {}
        }
        panel(body.into(), t)
    }

    /// The widget "Use" changes for a look of these kinds: the one picked last
    /// if it can show it, else the first that can.
    fn target_for(&self, kinds: &[String]) -> Option<&widgets::Placed> {
        let selected = self.selected.and_then(|id| self.widgets.placed.iter().find(|placed| placed.id == id)).filter(|placed| kinds.contains(&placed.kind));
        selected.or_else(|| self.widgets.placed.iter().find(|placed| kinds.contains(&placed.kind)))
    }

    fn look_card<'a>(&'a self, skin: &'static paperloom_skin::Skin, t: theme::Tokens) -> Element<'a, Message> {
        let target = self.target_for(&skin.kinds);
        // The kind it is shown as: the filter's, else its widget's, else its first.
        let kind = if skin.kinds.contains(&self.filter) {
            self.filter.clone()
        } else {
            target.map(|placed| placed.kind.clone()).or_else(|| skin.kinds.first().cloned()).unwrap_or_default()
        };
        let in_use = target.is_some_and(|placed| widgets_looks::skin_of(placed).is_some_and(|current| current.id == skin.id));
        let mut actions: Vec<Element<'a, Message>> = Vec::new();
        if let Some(placed) = target.filter(|_| !in_use) {
            actions.push(button(ui::label(format!("Use on {}", self.title_of(placed))).color(ui::on_accent())).style(ui::primary).on_press(Message::UseLook(skin.id.clone())).into());
        }
        actions.push(button(ui::label("Add")).style(ui::secondary).on_press(Message::AddWithLook(kind.clone(), skin.id.clone())).into());
        // Installed and imported looks can be removed again.
        if let Some(folder) = skin.dir.as_ref().and_then(|dir| dir.file_name()).and_then(|name| name.to_str()) {
            actions.push(button(ui::label("Remove")).style(ui::ghost).on_press(Message::RemoveLook(folder.to_string())).into());
        }
        let detail = match target.filter(|_| in_use) {
            Some(placed) => format!("In use on your {}", self.title_of(placed)),
            None => format!("by {}", skin.author),
        };
        card(self.previews.card(&skin.id, &kind), skin.name.clone(), detail, actions, None, in_use, t)
    }
}
