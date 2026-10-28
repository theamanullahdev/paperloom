//! Widgets: the desktop clocks and meters drawn by `paperloom-widgets`, a
//! process of their own (the wallpaper engine never draws them). This page
//! is where they are switched on, added, removed and set up. My widgets is a
//! workspace — the placed ones on the left, the chosen one's settings on the
//! right — and Browse lists everything that can be added.
//!
//! Everything is written to `widgets.toml`; the widgets process notices the
//! change within a couple of seconds, so there is no other channel to keep
//! in step.

use std::time::Duration;

use iced::widget::{button, column, container, row, slider, text, toggler, Space};
use iced::{Alignment, Background, Border, Element, Length, Subscription, Task};

use paperloom_common::widgets::{self, Widgets, BUILT_IN, MAX_PLACED};

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
    X(u32, f32),
    Y(u32, f32),
    Drag(u32, bool),
    Menu(u32, bool),
    ResetLayout,
    Browse,
    Tick,
}

pub struct State {
    widgets: Widgets,
    selected: Option<u32>,
    running: bool,
    /// Why the widgets process couldn't be started, if it couldn't.
    problem: Option<String>,
    /// The primary screen's size, which the Layout page draws to scale.
    screen: (u32, u32),
}

/// Which sub-page the widgets body shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Mine,
    Browse,
    Layout,
}

impl State {
    pub fn new() -> Self {
        let widgets = Widgets::load();
        let selected = widgets.placed.first().map(|placed| placed.id);
        Self { widgets, selected, running: widgets::is_running(), problem: None, screen: paperloom_common::primary_size().unwrap_or((1920, 1080)) }
    }

    /// Re-reads the file (another window, the tray, or a reset may have changed it).
    pub fn open(&mut self) {
        self.widgets = Widgets::load();
        self.running = widgets::is_running();
        if self.selected.is_none_or(|id| !self.widgets.placed.iter().any(|placed| placed.id == id)) {
            self.selected = self.widgets.placed.first().map(|placed| placed.id);
        }
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

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick => {
                self.running = widgets::is_running();
                return Task::none();
            }
            Message::Browse => return Task::none(),
            Message::SetEnabled(enabled) => {
                self.set_enabled(enabled);
                return Task::none();
            }
            Message::Add(kind) => {
                self.widgets = Widgets::load();
                if let Some(id) = self.widgets.add(kind) {
                    self.selected = Some(id);
                    self.widgets.enabled = true;
                } else {
                    self.problem = Some(format!("You can place up to {MAX_PLACED} widgets."));
                    return Task::none();
                }
            }
            Message::Remove(id) => {
                self.widgets.remove(id);
                if self.selected == Some(id) {
                    self.selected = self.widgets.placed.first().map(|placed| placed.id);
                }
            }
            Message::Select(id) => {
                self.selected = Some(id);
                return Task::none();
            }
            Message::Moved(id, x, y) => {
                if let Some(target) = self.widgets.placed.iter_mut().find(|target| target.id == id) {
                    target.x = x;
                    target.y = y;
                }
                return Task::none();
            }
            Message::Dropped => {}
            Message::ResetLayout => {
                let kinds: Vec<String> = self.widgets.placed.iter().map(|placed| placed.kind.clone()).collect();
                self.widgets.placed.clear();
                for kind in kinds {
                    self.widgets.add(&kind);
                }
                self.selected = self.widgets.placed.first().map(|placed| placed.id);
            }
            Message::Scale(id, value) | Message::Opacity(id, value) | Message::X(id, value) | Message::Y(id, value) => {
                if let Some(target) = self.widgets.placed.iter_mut().find(|target| target.id == id) {
                    match message {
                        Message::Scale(..) => target.scale = value.clamp(0.5, 3.0),
                        Message::Opacity(..) => target.opacity = value.clamp(0.1, 1.0),
                        Message::X(..) => target.x = value.clamp(0.0, 0.95),
                        _ => target.y = value.clamp(0.0, 0.95),
                    }
                }
            }
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
        let state = if self.running { "Showing on the desktop" } else if self.widgets.enabled { "Starting…" } else { "Off" };
        row![
            text(state).size(13).color(if self.running { t.success } else { t.muted_foreground }),
            toggler(self.widgets.enabled).label("Show widgets").on_toggle(Message::SetEnabled),
        ]
        .spacing(16)
        .align_y(Alignment::Center)
        .into()
    }

    pub fn body<'a>(&'a self, mode: Mode, t: theme::Tokens) -> Element<'a, Message> {
        let mut page = column![].spacing(12);
        if let Some(problem) = &self.problem {
            page = page.push(text(problem.as_str()).size(13).color(t.destructive));
        }
        page = page.push(match mode {
            Mode::Browse => self.browse(t),
            Mode::Mine => self.mine(t),
            Mode::Layout => self.layout(t),
        });
        ui::scroll_page(page).height(Length::Fill).into()
    }

    fn layout<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        if self.widgets.placed.is_empty() {
            return self.mine(t);
        }
        column![
            text("Drag a widget to move it. It snaps to the edges and the middle, and is saved when you let go.").size(13).color(t.muted_foreground),
            super::widgets_layout::view(&self.widgets.placed, self.selected, self.screen, t),
        ]
        .spacing(12)
        .into()
    }

    fn browse<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let mut cards = column![].spacing(8);
        for entry in BUILT_IN {
            let placed = self.widgets.placed.iter().filter(|placed| placed.kind == entry.kind).count();
            let action: Element<'a, Message> = button(ui::label("Add to desktop")).style(ui::primary).on_press(Message::Add(entry.kind)).into();
            cards = cards.push(panel(
                row![
                    column![text(entry.name).size(16).color(t.foreground), text(entry.about).size(13).color(t.muted_foreground)].spacing(2),
                    Space::new().width(Length::Fill),
                    text(if placed > 0 { format!("{placed} on your desktop") } else { String::new() }).size(12).color(t.muted_foreground),
                    action,
                ]
                .spacing(12)
                .align_y(Alignment::Center)
                .into(),
                t,
            ));
        }
        cards.into()
    }

    fn mine<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        if self.widgets.placed.is_empty() {
            return panel(
                column![
                    text("No widgets on your desktop yet").size(16).color(t.foreground),
                    text("Add a clock or a meter and it appears on the desktop, above the wallpaper.").size(13).color(t.muted_foreground),
                    button(ui::label("Browse widgets")).style(ui::primary).on_press(Message::Browse),
                ]
                .spacing(10)
                .into(),
                t,
            );
        }

        let mut list = column![].spacing(6);
        for placed in &self.widgets.placed {
            let name = widgets::entry(&placed.kind).map_or(placed.kind.as_str(), |entry| entry.name);
            let chosen = self.selected == Some(placed.id);
            list = list.push(
                button(row![ui::label(name).color(if chosen { ui::on_accent() } else { t.foreground })].padding([4, 6]))
                    .width(Length::Fill)
                    .style(if chosen { ui::primary } else { ui::secondary })
                    .on_press(Message::Select(placed.id)),
            );
        }
        let sidebar = column![
            list,
            Space::new().height(Length::Fixed(8.0)),
            button(ui::label("Add another")).style(ui::secondary).width(Length::Fill).on_press(Message::Browse),
            button(ui::label("Reset layout")).style(ui::ghost).width(Length::Fill).on_press(Message::ResetLayout),
        ]
        .spacing(6)
        .width(Length::Fixed(220.0));

        let settings: Element<'a, Message> = match self.selected.and_then(|id| self.widgets.placed.iter().find(|placed| placed.id == id)) {
            Some(placed) => self.settings_of(placed, t),
            None => text("Pick a widget on the left.").size(13).color(t.muted_foreground).into(),
        };
        row![sidebar, container(settings).width(Length::Fill)].spacing(16).into()
    }

    fn settings_of<'a>(&'a self, placed: &'a widgets::Placed, t: theme::Tokens) -> Element<'a, Message> {
        let id = placed.id;
        let name = widgets::entry(&placed.kind).map_or(placed.kind.as_str(), |entry| entry.name);
        let percent = |value: f32| text(format!("{:.0}%", value * 100.0)).size(13).color(t.muted_foreground);
        let line = |label: &'static str, control: Element<'a, Message>, value: Element<'a, Message>| -> Element<'a, Message> {
            row![text(label).size(13).color(t.foreground).width(Length::Fixed(90.0)), control, value].spacing(12).align_y(Alignment::Center).into()
        };
        panel(
            column![
                text(name).size(16).color(t.foreground),
                line("Size", slider(0.5..=2.0, placed.scale, move |value| Message::Scale(id, value)).step(0.05_f32).into(), percent(placed.scale).into()),
                line("Opacity", slider(0.1..=1.0, placed.opacity, move |value| Message::Opacity(id, value)).step(0.05_f32).into(), percent(placed.opacity).into()),
                line("Left", slider(0.0..=0.95, placed.x, move |value| Message::X(id, value)).step(0.01_f32).into(), percent(placed.x).into()),
                line("Top", slider(0.0..=0.95, placed.y, move |value| Message::Y(id, value)).step(0.01_f32).into(), percent(placed.y).into()),
                column![
                    toggler(placed.drag).label("Drag it on the desktop").on_toggle(move |value| Message::Drag(id, value)),
                    toggler(placed.menu).label("Right-click menu").on_toggle(move |value| Message::Menu(id, value)),
                    text(if placed.interactive() {
                        "It sits above your desktop icons so it can be clicked. Turn both off and it sits below them, and every click goes through to the desktop."
                    } else {
                        "Clicks go through to the desktop and it sits below your desktop icons. Move it from Edit layout."
                    })
                    .size(12)
                    .color(t.muted_foreground),
                ]
                .spacing(8),
                row![Space::new().width(Length::Fill), button(ui::label("Remove from desktop")).style(ui::danger).on_press(Message::Remove(id))],
            ]
            .spacing(14)
            .into(),
            t,
        )
    }
}

fn panel<'a>(content: Element<'a, Message>, t: theme::Tokens) -> Element<'a, Message> {
    container(content)
        .padding(14)
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(t.surface)),
            border: Border { color: t.border, width: 1.0, radius: 6.0.into() },
            ..container::Style::default()
        })
        .into()
}
