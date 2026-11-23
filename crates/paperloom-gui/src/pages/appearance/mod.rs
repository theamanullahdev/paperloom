//! Appearance: the theme Paperloom's own window wears.
//!
//! One workspace: themes on the left (the ones on this computer, or the
//! online library), the chosen theme on the right. A downloaded or imported
//! theme lands in "On this computer". Editing a theme edits its file,
//! through a form (`editor.rs`).

mod editor;
mod pick;
mod preview;

use std::collections::HashMap;
use std::path::PathBuf;

use iced::widget::{button, column, container, image, row, scrollable, text, text_input, Space};
use iced::{Alignment, Background, Border, Color, Element, Length, Task};

use paperloom_common::{Settings, ThemeMode};
use paperloom_fetch::looks::{Index, Pack};
use paperloom_fetch::themes::{self, Imported};
use paperloom_theme::{Listed, Theme};

use crate::pages::widgets_looks::blocking;
use crate::{theme, ui};

pub use preview::mock;

const MODES: [ThemeMode; 3] = [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Mine,
    Download,
}

#[derive(Debug, Clone)]
pub enum Message {
    TabSelected(Tab),
    Search(String),
    Mode(ThemeMode),
    Select(String),
    Use(String),
    Edit(String),
    Duplicate(String),
    Export(String),
    Remove(String),
    ConfirmRemove,
    CancelRemove,
    ImportFile,
    ImportFolder,
    Imported(Result<Imported, String>),
    CloseNotice,
    // --- Online library
    OnlineLoaded(Result<(Index, String), String>),
    OnlinePreview(String, Option<Vec<u8>>),
    OnlineSelect(String),
    Install(String),
    Installed(String, Result<(), String>),
    Reload,
    // --- Editor
    Editor(editor::Message),
    // --- "Keep this theme?" (handled by the app)
    Keep,
    Revert,
    Restart,
}

/// What the app should do after a message.
pub enum Outcome {
    None,
    /// Switch to this theme (asks "Keep this theme?" when it moves things).
    Apply(String),
    /// Draw with this unsaved theme for a moment (the editor's Preview).
    Preview(Box<Theme>, Option<PathBuf>),
    /// Back to the saved theme.
    EndPreview,
}

/// A result to read: what an import did, or why something failed.
struct Notice {
    title: String,
    lines: Vec<String>,
}

pub struct State {
    themes: Vec<Listed>,
    tab: Tab,
    selected: String,
    search: String,
    editor: Option<editor::State>,
    notice: Option<Notice>,
    confirm_remove: Option<String>,
    // Online library
    online: Option<(Index, String)>,
    online_loading: bool,
    online_error: Option<String>,
    online_selected: Option<String>,
    previews: HashMap<String, image::Handle>,
    installing: Option<String>,
    /// The body font the window started with; another one shows after a restart.
    started_with: String,
}

impl State {
    pub fn new() -> Self {
        let active = theme::active();
        Self {
            themes: paperloom_theme::list(&paperloom_common::themes_dir()),
            tab: Tab::Mine,
            selected: active.id().to_string(),
            search: String::new(),
            editor: None,
            notice: None,
            confirm_remove: None,
            online: None,
            online_loading: false,
            online_error: None,
            online_selected: None,
            previews: HashMap::new(),
            installing: None,
            started_with: active.body_family.clone(),
        }
    }

    /// The page was opened, or themes changed on disk.
    /// Imports a theme file opened from outside the window (a double-click).
    pub fn import_path(&mut self, path: std::path::PathBuf) -> Task<Message> {
        blocking(move || themes::import_path(&path, &paperloom_common::themes_dir()), Message::Imported)
    }

    pub fn refresh(&mut self) {
        self.themes = paperloom_theme::list(&paperloom_common::themes_dir());
        if !self.themes.iter().any(|listed| listed.theme.id == self.selected) {
            self.selected = theme::active().id().to_string();
        }
    }

    /// The theme in use names a body font the window didn't start with.
    fn needs_restart(&self) -> bool {
        theme::active().body_family != self.started_with
    }

    fn find(&self, id: &str) -> Option<&Listed> {
        self.themes.iter().find(|listed| listed.theme.id == id)
    }

    fn load_online(&mut self) -> Task<Message> {
        if self.online_loading {
            return Task::none();
        }
        self.online_loading = true;
        self.online_error = None;
        blocking(|| themes::fetch_index().map(|(index, base)| (index, base.to_string())), Message::OnlineLoaded)
    }

    /// A free id and name for a copy of `source`.
    fn copy_of(&self, source: &Theme) -> Theme {
        let base = paperloom_theme::folder_name(&source.id);
        let mut number = 1;
        let id = loop {
            let candidate = if number == 1 { format!("{base}-copy") } else { format!("{base}-copy-{number}") };
            if self.find(&candidate).is_none() {
                break candidate;
            }
            number += 1;
        };
        Theme { id, name: format!("{} (copy)", source.name), author: String::new(), ..source.clone() }
    }

    /// Writes a copy of theme `id` (with its fonts and pictures) and returns the copy's id.
    fn duplicate(&mut self, id: &str) -> Result<String, String> {
        let source = self.find(id).cloned().ok_or("That theme is gone.")?;
        let copy = self.copy_of(&source.theme);
        let themes_dir = paperloom_common::themes_dir();
        let dir = paperloom_theme::save(&themes_dir, &copy).map_err(|err| err.to_string())?;
        if let Some(from) = source.dir() {
            copy_assets(from, &dir);
        }
        self.refresh();
        Ok(copy.id)
    }

    pub fn update(&mut self, settings: &mut Settings, message: Message) -> (Task<Message>, Outcome) {
        match message {
            Message::TabSelected(tab) => {
                self.tab = tab;
                if tab == Tab::Download && self.online.is_none() {
                    return (self.load_online(), Outcome::None);
                }
            }
            Message::Search(value) => self.search = value,
            Message::Mode(mode) => {
                settings.theme = mode;
                let _ = paperloom_common::save(settings);
            }
            Message::Select(id) => self.selected = id,
            Message::Use(id) => return (Task::none(), Outcome::Apply(id)),
            Message::Edit(id) => {
                // A theme Paperloom ships with is never changed: edit a copy of it.
                let id = match self.find(&id) {
                    Some(listed) if listed.built_in() => match self.duplicate(&id) {
                        Ok(copy) => copy,
                        Err(why) => {
                            self.notice = Some(Notice { title: "The theme couldn't be copied".into(), lines: vec![why] });
                            return (Task::none(), Outcome::None);
                        }
                    },
                    _ => id,
                };
                if let Some(listed) = self.find(&id).cloned() {
                    self.selected = id;
                    self.editor = Some(editor::State::new(listed));
                }
            }
            Message::Duplicate(id) => match self.duplicate(&id) {
                Ok(copy) => self.selected = copy,
                Err(why) => self.notice = Some(Notice { title: "The theme couldn't be copied".into(), lines: vec![why] }),
            },
            Message::Export(id) => {
                if let Some(dir) = self.find(&id).and_then(|listed| listed.dir().map(PathBuf::from)) {
                    let name = format!("{}.paperloomtheme", paperloom_theme::folder_name(&id));
                    if let Some(to) = rfd::FileDialog::new().set_title("Save the theme as a file").set_file_name(name).save_file() {
                        self.notice = Some(match themes::export(&dir, &to) {
                            Ok(path) => Notice { title: "Theme saved as a file".into(), lines: vec![path.display().to_string(), "Anyone can add it with Import on this page.".into()] },
                            Err(why) => Notice { title: "The theme couldn't be saved".into(), lines: vec![why] },
                        });
                    }
                }
            }
            Message::Remove(id) => self.confirm_remove = Some(id),
            Message::CancelRemove => self.confirm_remove = None,
            Message::ConfirmRemove => {
                if let Some(id) = self.confirm_remove.take() {
                    let was_active = theme::active().id() == id;
                    let _ = paperloom_theme::remove(&paperloom_common::themes_dir(), &id);
                    self.refresh();
                    if was_active {
                        return (Task::none(), Outcome::Apply(String::new()));
                    }
                }
            }
            Message::ImportFile => {
                if let Some(path) = rfd::FileDialog::new().set_title("Import a theme").add_filter("Paperloom themes", &["paperloomtheme", "toml"]).pick_file() {
                    return (blocking(move || themes::import_path(&path, &paperloom_common::themes_dir()), Message::Imported), Outcome::None);
                }
            }
            Message::ImportFolder => {
                if let Some(path) = rfd::FileDialog::new().set_title("Import a theme folder").pick_folder() {
                    return (blocking(move || themes::import_path(&path, &paperloom_common::themes_dir()), Message::Imported), Outcome::None);
                }
            }
            Message::Imported(result) => {
                self.refresh();
                self.notice = Some(match result {
                    Ok(imported) => {
                        self.tab = Tab::Mine;
                        self.selected = imported.id.clone();
                        let mut lines = vec![format!("“{}” is now in On this computer.", imported.name)];
                        lines.extend(imported.warnings);
                        Notice { title: "Theme imported".into(), lines }
                    }
                    Err(why) => Notice { title: "This theme couldn't be imported".into(), lines: vec![why] },
                });
            }
            Message::CloseNotice => self.notice = None,
            Message::Reload => {
                self.online = None;
                return (self.load_online(), Outcome::None);
            }
            Message::OnlineLoaded(result) => {
                self.online_loading = false;
                match result {
                    Ok((index, base)) => {
                        let tasks: Vec<Task<Message>> = index
                            .packs
                            .iter()
                            .filter(|pack| !self.previews.contains_key(&pack.id))
                            .map(|pack| {
                                let (base, pack, id) = (base.clone(), pack.clone(), pack.id.clone());
                                blocking(move || themes::fetch_preview(&base, &pack).ok(), move |bytes| Message::OnlinePreview(id.clone(), bytes))
                            })
                            .collect();
                        self.online_selected = self.online_selected.take().or_else(|| index.packs.first().map(|pack| pack.id.clone()));
                        self.online = Some((index, base));
                        return (Task::batch(tasks), Outcome::None);
                    }
                    Err(why) => self.online_error = Some(why),
                }
            }
            Message::OnlinePreview(id, bytes) => {
                if let Some(bytes) = bytes {
                    self.previews.insert(id, image::Handle::from_bytes(bytes));
                }
            }
            Message::OnlineSelect(id) => self.online_selected = Some(id),
            Message::Install(id) => {
                if let Some((index, base)) = &self.online {
                    if let Some(pack) = index.packs.iter().find(|pack| pack.id == id).cloned() {
                        self.installing = Some(id.clone());
                        let base = base.clone();
                        return (blocking(move || themes::install(&base, &pack, &paperloom_common::themes_dir()), move |result| Message::Installed(id.clone(), result)), Outcome::None);
                    }
                }
            }
            Message::Installed(id, result) => {
                self.installing = None;
                self.refresh();
                match result {
                    Ok(()) => {
                        self.tab = Tab::Mine;
                        self.selected = id;
                    }
                    Err(why) => self.notice = Some(Notice { title: "The theme couldn't be downloaded".into(), lines: vec![why] }),
                }
            }
            Message::Editor(message) => {
                let Some(editor) = self.editor.as_mut() else { return (Task::none(), Outcome::None) };
                let (task, outcome) = editor.update(message);
                let task = task.map(Message::Editor);
                return match outcome {
                    editor::Outcome::None => (task, Outcome::None),
                    editor::Outcome::Preview(draft, dir) => (task, Outcome::Preview(draft, dir)),
                    editor::Outcome::Closed => {
                        self.editor = None;
                        (task, Outcome::EndPreview)
                    }
                    editor::Outcome::Saved(id) => {
                        self.editor = None;
                        self.refresh();
                        self.selected = id.clone();
                        // Saving the theme in use shows the change at once.
                        if settings.app_theme == id {
                            (task, Outcome::Apply(id))
                        } else {
                            (task, Outcome::EndPreview)
                        }
                    }
                };
            }
            // The app answers these (it owns the countdown and the restart).
            Message::Keep | Message::Revert | Message::Restart => {}
        }
        (Task::none(), Outcome::None)
    }

    pub fn header<'a>(&'a self, settings: &Settings, t: theme::Tokens) -> Element<'a, Message> {
        if self.editor.is_some() {
            return Space::new().into();
        }
        row![
            ui::segmented(MODES.iter().map(|mode| (mode.to_string(), settings.theme == *mode, Message::Mode(*mode))).collect(), t),
            button(ui::label("Import a theme").size(13)).style(ui::secondary).padding([7, 12]).on_press(Message::ImportFile),
            button(ui::label("Import a folder").size(13)).style(ui::secondary).padding([7, 12]).on_press(Message::ImportFolder),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    }

    pub fn body<'a>(&'a self, settings: &'a Settings, dark: bool, t: theme::Tokens) -> Element<'a, Message> {
        if let Some(editor) = &self.editor {
            return editor.view(dark, t).map(Message::Editor);
        }
        iced::widget::responsive(move |size| {
            let list = self.list(settings, dark, t);
            let detail = match self.tab {
                Tab::Mine => self.detail(settings, t),
                Tab::Download => self.online_detail(t),
            };
            if size.width >= 760.0 {
                row![container(list).width(Length::Fixed(300.0)).height(Length::Fill), container(detail).width(Length::Fill).height(Length::Fill)].spacing(16).height(Length::Fill).into()
            } else {
                ui::scroll_page(column![container(list).height(Length::Fixed(360.0)), container(detail).height(Length::Fixed(560.0))].spacing(16)).height(Length::Fill).into()
            }
        })
        .into()
    }

    /// Left: the two lists behind one tab bar, with a search box.
    fn list<'a>(&'a self, settings: &Settings, dark: bool, t: theme::Tokens) -> Element<'a, Message> {
        let tabs = ui::segmented(vec![("On this computer".to_string(), self.tab == Tab::Mine, Message::TabSelected(Tab::Mine)), ("Download".to_string(), self.tab == Tab::Download, Message::TabSelected(Tab::Download))], t);
        let needle = self.search.trim().to_lowercase();
        let mut rows = column![].spacing(4);
        match self.tab {
            Tab::Mine => {
                let in_use = theme_in_use(settings);
                for listed in self.themes.iter().filter(|listed| needle.is_empty() || listed.theme.name.to_lowercase().contains(&needle)) {
                    let theme = &listed.theme;
                    let note = if theme.id == in_use { "In use" } else if listed.built_in() { "Built in" } else { "" };
                    rows = rows.push(entry(mock(theme, dark, 84.0), &theme.name, &theme.author, note, theme.id == self.selected, Message::Select(theme.id.clone()), t));
                }
            }
            Tab::Download => match (&self.online, &self.online_error) {
                (Some((index, _)), _) => {
                    for pack in index.packs.iter().filter(|pack| needle.is_empty() || pack.name.to_lowercase().contains(&needle)) {
                        let thumb: Element<'a, Message> = match self.previews.get(&pack.id) {
                            Some(handle) => image(handle.clone()).width(84).height(52).content_fit(iced::ContentFit::Cover).into(),
                            None => container(Space::new()).width(84).height(52).style(move |_theme| container::Style { background: Some(Background::Color(t.muted)), ..container::Style::default() }).into(),
                        };
                        let note = if self.find(&pack.id).is_some() { "On this computer" } else { "" };
                        rows = rows.push(entry(thumb, &pack.name, &pack.author, note, self.online_selected.as_deref() == Some(pack.id.as_str()), Message::OnlineSelect(pack.id.clone()), t));
                    }
                    if index.packs.is_empty() {
                        rows = rows.push(text("The online library has no themes right now.").size(13).color(t.muted_foreground));
                    }
                }
                (None, Some(error)) => {
                    rows = rows.push(text(format!("The online library couldn't be read: {error}")).size(13).color(t.destructive));
                    rows = rows.push(button(ui::label("Try again").size(13)).style(ui::secondary).padding([6, 12]).on_press(Message::Reload));
                }
                (None, None) => rows = rows.push(text("Reading the online library…").size(13).color(t.muted_foreground)),
            },
        }
        let search = text_input("Search themes…", &self.search).on_input(Message::Search).padding(7).size(13);
        panel(column![tabs, search, scrollable(rows.padding(iced::Padding { right: 10.0, ..iced::Padding::ZERO })).height(Length::Fill)].spacing(10).into(), t)
    }

    /// Right: the chosen theme on this computer.
    fn detail<'a>(&'a self, settings: &Settings, t: theme::Tokens) -> Element<'a, Message> {
        let Some(listed) = self.find(&self.selected) else {
            return panel(text("Choose a theme on the left.").size(14).color(t.muted_foreground).into(), t);
        };
        let theme = &listed.theme;
        let id = theme.id.clone();
        let in_use = theme_in_use(settings) == id;

        let mut actions = row![].spacing(8).align_y(Alignment::Center);
        actions = actions.push(button(ui::label(if in_use { "In use" } else { "Use this theme" }).size(14)).style(ui::primary).padding([8, 16]).on_press_maybe((!in_use).then(|| Message::Use(id.clone()))));
        actions = actions.push(button(ui::label(if listed.built_in() { "Edit a copy" } else { "Edit" }).size(14)).style(ui::secondary).padding([8, 14]).on_press(Message::Edit(id.clone())));
        actions = actions.push(button(ui::label("Duplicate").size(14)).style(ui::secondary).padding([8, 14]).on_press(Message::Duplicate(id.clone())));
        if !listed.built_in() {
            actions = actions.push(button(ui::label("Save as a file").size(14)).style(ui::secondary).padding([8, 14]).on_press(Message::Export(id.clone())));
            actions = actions.push(button(ui::label("Remove").size(14)).style(ui::danger).padding([8, 14]).on_press(Message::Remove(id.clone())));
        }

        let modes = match (&theme.colors.dark, &theme.colors.light) {
            (Some(_), None) => "Dark colours only",
            (None, Some(_)) => "Light colours only",
            _ => "Light and dark",
        };
        let nav = format!("{} · {}", theme.layout.nav, theme.layout.nav_style);
        let fonts = match (theme.fonts.body.is_empty(), theme.fonts.display.is_empty()) {
            (true, true) => "Paperloom's own".to_string(),
            _ => [theme.fonts.display.as_str(), theme.fonts.body.as_str()].iter().filter(|name| !name.is_empty()).cloned().collect::<Vec<_>>().join(" · "),
        };
        let mut facts = column![fact("Colours", modes.to_string(), t), fact("Navigation", nav, t), fact("Fonts", fonts, t)].spacing(8);
        if !theme.layer.is_empty() {
            facts = facts.push(fact("Picture layers", theme.layer.len().to_string(), t));
        }
        if let Some(dir) = listed.dir() {
            facts = facts.push(fact("Folder", dir.display().to_string(), t));
        }

        let mut content = column![
            row![mock(theme, true, 300.0), mock(theme, false, 300.0)].spacing(12).wrap(),
            text(theme.name.clone()).font(theme::display_font()).size(22).color(t.foreground),
        ]
        .spacing(12);
        if !theme.author.is_empty() {
            content = content.push(text(format!("by {}", theme.author)).size(13).color(t.muted_foreground));
        }
        if !theme.description.is_empty() {
            content = content.push(text(theme.description.clone()).size(14).color(t.foreground));
        }
        content = content.push(actions.wrap().vertical_spacing(8));
        if in_use && self.needs_restart() {
            content = content.push(
                row![text("This theme's text font shows after a restart.").size(13).color(t.muted_foreground), button(ui::label("Restart now").size(13)).style(ui::secondary).padding([5, 12]).on_press(Message::Restart)]
                    .spacing(10)
                    .align_y(Alignment::Center),
            );
        }
        content = content.push(ui::rule(t)).push(facts);
        panel(scrollable(content.padding(iced::Padding { right: 12.0, ..iced::Padding::ZERO })).height(Length::Fill).into(), t)
    }

    /// Right: the chosen theme of the online library.
    fn online_detail<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let pack: Option<&Pack> = self.online.as_ref().and_then(|(index, _)| index.packs.iter().find(|pack| Some(pack.id.as_str()) == self.online_selected.as_deref()));
        let Some(pack) = pack else {
            return panel(text("Themes made by others. Pick one on the left to see it; a downloaded theme appears in On this computer.").size(14).color(t.muted_foreground).into(), t);
        };
        let picture: Element<'a, Message> = match self.previews.get(&pack.id) {
            Some(handle) => image(handle.clone()).width(Length::Fill).content_fit(iced::ContentFit::Contain).into(),
            None => container(Space::new()).width(Length::Fill).height(220).style(move |_theme| container::Style { background: Some(Background::Color(t.muted)), ..container::Style::default() }).into(),
        };
        let have = self.find(&pack.id).is_some();
        let busy = self.installing.as_deref() == Some(pack.id.as_str());
        let label = if busy { "Downloading…" } else if have { "Download again" } else { "Download" };
        let mut actions = row![button(ui::label(label).size(14)).style(ui::primary).padding([8, 16]).on_press_maybe((!busy).then(|| Message::Install(pack.id.clone())))].spacing(8).align_y(Alignment::Center);
        if have {
            actions = actions.push(button(ui::label("Use this theme").size(14)).style(ui::secondary).padding([8, 14]).on_press(Message::Use(pack.id.clone())));
        }
        let mut content = column![container(picture).max_width(640.0), text(pack.name.clone()).font(theme::display_font()).size(22).color(t.foreground)].spacing(12);
        if !pack.author.is_empty() {
            content = content.push(text(format!("by {}", pack.author)).size(13).color(t.muted_foreground));
        }
        if !pack.description.is_empty() {
            content = content.push(text(pack.description.clone()).size(14).color(t.foreground));
        }
        content = content.push(actions).push(ui::rule(t)).push(fact("Download size", format!("{} KB", pack.download_size().div_ceil(1024)), t));
        panel(scrollable(content.padding(iced::Padding { right: 12.0, ..iced::Padding::ZERO })).height(Length::Fill).into(), t)
    }

    /// Pop-ups over the page: an import's result, or "remove this theme?".
    pub fn overlay<'a>(&'a self, t: theme::Tokens) -> Option<Element<'a, Message>> {
        if let Some(id) = &self.confirm_remove {
            let name = self.find(id).map(|listed| listed.theme.name.clone()).unwrap_or_else(|| id.clone());
            let content = column![
                text(format!("Remove “{name}”?")).font(theme::display_font()).size(18).color(t.foreground),
                text("Its folder is deleted from this computer. If it is the theme in use, Paperloom's own takes over.").size(13).color(t.muted_foreground),
                row![button(ui::label("Remove")).style(ui::danger).padding([8, 16]).on_press(Message::ConfirmRemove), button(ui::label("Keep it")).style(ui::secondary).padding([8, 16]).on_press(Message::CancelRemove)].spacing(8),
            ]
            .spacing(12);
            return Some(ui::modal(content, 440.0, Message::CancelRemove, t));
        }
        if let Some(editor) = &self.editor {
            if let Some(picker) = editor.overlay(t) {
                return Some(picker.map(Message::Editor));
            }
        }
        let notice = self.notice.as_ref()?;
        let mut content = column![text(notice.title.clone()).font(theme::display_font()).size(18).color(t.foreground)].spacing(10);
        for line in &notice.lines {
            content = content.push(text(line.clone()).size(13).color(t.foreground));
        }
        content = content.push(button(ui::label("OK")).style(ui::primary).padding([8, 20]).on_press(Message::CloseNotice));
        Some(ui::modal(content, 520.0, Message::CloseNotice, t))
    }
}

/// "Keep this theme?": shown by the app after a theme that moves things
/// was applied; it goes back by itself when the seconds run out.
pub fn keep_question<'a>(seconds_left: u8, t: theme::Tokens) -> Element<'a, Message> {
    let content = column![
        text("Keep this theme?").font(theme::display_font()).size(18).color(t.foreground),
        text(format!("It moved things around. Going back to the one before in {seconds_left} seconds.")).size(13).color(t.muted_foreground),
        row![button(ui::label("Keep it")).style(ui::primary).padding([8, 18]).on_press(Message::Keep), button(ui::label("Go back")).style(ui::secondary).padding([8, 18]).on_press(Message::Revert)].spacing(8),
    ]
    .spacing(12);
    ui::modal(content, 420.0, Message::Revert, t)
}

/// The id of the theme the settings name (empty = Paperloom's own).
fn theme_in_use(settings: &Settings) -> String {
    if settings.app_theme.is_empty() {
        paperloom_theme::DEFAULT_ID.to_string()
    } else {
        settings.app_theme.clone()
    }
}

/// Copies a theme's fonts and pictures (everything but its `theme.toml`).
fn copy_assets(from: &std::path::Path, to: &std::path::Path) {
    for folder in ["fonts", "images"] {
        let Ok(entries) = std::fs::read_dir(from.join(folder)) else { continue };
        let _ = std::fs::create_dir_all(to.join(folder));
        for entry in entries.flatten().filter(|entry| entry.path().is_file()) {
            let _ = std::fs::copy(entry.path(), to.join(folder).join(entry.file_name()));
        }
    }
}

fn panel<'a>(content: Element<'a, Message>, t: theme::Tokens) -> Element<'a, Message> {
    container(content)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme| container::Style { background: Some(Background::Color(t.surface)), border: Border { color: t.border, width: theme::border_width(), radius: theme::radius_card().into() }, ..container::Style::default() })
        .into()
}

fn fact<'a>(name: &'a str, value: String, t: theme::Tokens) -> Element<'a, Message> {
    row![text(name).size(13).color(t.muted_foreground).width(Length::Fixed(120.0)), text(value).size(13).color(t.foreground)].spacing(8).into()
}

/// One row of a list: thumbnail, name, author, a short note.
fn entry<'a>(thumb: Element<'a, Message>, name: &str, author: &str, note: &'a str, selected: bool, on_press: Message, t: theme::Tokens) -> Element<'a, Message> {
    let mut words = column![text(name.to_string()).size(14).color(if selected { t.primary } else { t.foreground })].spacing(2);
    if !author.is_empty() {
        words = words.push(text(author.to_string()).size(12).color(t.muted_foreground));
    }
    if !note.is_empty() {
        words = words.push(text(note).size(11).color(t.primary));
    }
    button(row![thumb, words].spacing(10).align_y(Alignment::Center))
        .padding(6)
        .width(Length::Fill)
        .on_press(on_press)
        .style(move |_theme, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: if selected { Some(Background::Color(Color { a: 0.12, ..t.primary })) } else if hovered { Some(Background::Color(t.muted)) } else { None },
                text_color: t.foreground,
                border: Border { radius: theme::radius_control().into(), ..Border::default() },
                ..button::Style::default()
            }
        })
        .into()
}
