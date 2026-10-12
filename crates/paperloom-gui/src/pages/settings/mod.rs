mod registry;

use std::time::{Duration, Instant};

use iced::widget::{column, container, mouse_area, pick_list, scrollable, text, text_input, toggler};
use iced::{Animation, Background, Border, Element, Length, Subscription, Task};

use paperloom_common::{ImportMode, Settings, ThemeMode};
pub use registry::SettingId;
use registry::ENTRIES;

use crate::theme;

const THEME_OPTIONS: &[ThemeMode] = &[ThemeMode::System, ThemeMode::Light, ThemeMode::Dark];
const IMPORT_MODE_OPTIONS: &[ImportMode] = &[
    ImportMode::CopyAndOptimize,
    ImportMode::CopyNoReencode,
    ImportMode::ReferenceInPlace,
];

pub struct State {
    search: String,
    /// The setting a search result most recently jumped to, *or* the one
    /// just toggled — both cases reuse the same fade as feedback (jump:
    /// "here it is", toggle: "that registered"), rather than tracking two
    /// separate animation states for what's visually the same effect.
    highlighted: Option<SettingId>,
    highlight_fade: Animation<bool>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            search: String::new(),
            highlighted: None,
            highlight_fade: Animation::new(false).quick(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    SearchChanged(String),
    ResultSelected(SettingId),
    Toggle(SettingId, bool),
    ImportModeSelected(ImportMode),
    ThemeSelected(ThemeMode),
    /// Ticks only while `highlight_fade` is actually animating — see
    /// `subscription`. Carries no data; it exists purely to trigger a
    /// redraw so `view` re-samples the animation at the new time.
    Tick,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    /// Only subscribes to frame ticks while a highlight is actively
    /// fading in, so idling on this page (the common case) costs nothing.
    pub fn subscription(&self) -> Subscription<Message> {
        if self.highlight_fade.is_animating(Instant::now()) {
            iced::time::every(Duration::from_millis(16)).map(|_| Message::Tick)
        } else {
            Subscription::none()
        }
    }

    fn flash(&mut self, id: SettingId) {
        self.highlighted = Some(id);
        self.highlight_fade = Animation::new(false).quick();
        self.highlight_fade.go_mut(true, Instant::now());
    }

    pub fn update(&mut self, settings: &mut Settings, message: Message) -> Task<Message> {
        match message {
            Message::SearchChanged(value) => self.search = value,
            Message::ResultSelected(id) => {
                self.search.clear();
                self.flash(id);
            }
            Message::Tick => {}
            Message::Toggle(id, value) => {
                match id {
                    SettingId::PauseFullscreenApp => settings.pause_on_fullscreen_app = value,
                    SettingId::PauseScreenLock => settings.pause_on_screen_lock = value,
                    SettingId::PauseBatterySaver => settings.pause_on_battery_saver = value,
                    SettingId::PauseWindowCovering => {
                        settings.pause_on_window_covering_desktop = value;
                    }
                    SettingId::Muted => settings.muted = value,
                    SettingId::ImportMode | SettingId::Theme => {}
                }
                self.flash(id);
                let _ = paperloom_common::save(settings);
            }
            Message::ImportModeSelected(mode) => {
                settings.import_mode = mode;
                self.flash(SettingId::ImportMode);
                let _ = paperloom_common::save(settings);
            }
            Message::ThemeSelected(theme) => {
                settings.theme = theme;
                self.flash(SettingId::Theme);
                let _ = paperloom_common::save(settings);
            }
        }
        Task::none()
    }

    pub fn header<'a>(&'a self) -> Element<'a, Message> {
        text_input("Search settings... try \"dark mode\"", &self.search)
            .on_input(Message::SearchChanged)
            .padding(8)
            .width(Length::Fixed(280.0))
            .into()
    }

    pub fn body<'a>(&'a self, settings: &'a Settings, t: theme::Tokens) -> Element<'a, Message> {
        if self.search.is_empty() {
            scrollable(self.full_list(settings, t)).into()
        } else {
            scrollable(self.search_results(t)).into()
        }
    }

    fn search_results<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let results: Vec<_> = ENTRIES.iter().filter(|entry| registry::matches(entry, &self.search)).collect();

        if results.is_empty() {
            return text("No matching settings.").into();
        }

        let mut list = column![].spacing(6);
        for entry in results {
            let row = card(text(format!("{}  ·  {}", entry.section, entry.label)).into(), t);
            list = list.push(mouse_area(row).on_press(Message::ResultSelected(entry.id)));
        }
        list.into()
    }

    fn full_list<'a>(&'a self, settings: &'a Settings, t: theme::Tokens) -> Element<'a, Message> {
        let now = Instant::now();
        let row = move |id: SettingId, content: Element<'a, Message>| {
            let fade = if self.highlighted == Some(id) { self.highlight_fade.interpolate(0.0_f32, 1.0_f32, now) } else { 0.0 };
            let base = card(content, t);
            if fade > 0.0 {
                container(base).style(move |_theme| container::Style {
                    background: Some(Background::Color(iced::Color { a: fade * 0.18, ..t.primary })),
                    border: Border { color: iced::Color { a: fade, ..t.primary }, width: 1.0, radius: 10.0.into() },
                    ..container::Style::default()
                })
                .into()
            } else {
                base
            }
        };

        column![
            section_title("Performance & idle behavior", t),
            row(
                SettingId::PauseFullscreenApp,
                toggler(settings.pause_on_fullscreen_app)
                    .label("Pause when a fullscreen app is focused")
                    .on_toggle(|v| Message::Toggle(SettingId::PauseFullscreenApp, v))
                    .into(),
            ),
            row(
                SettingId::PauseScreenLock,
                toggler(settings.pause_on_screen_lock)
                    .label("Pause when the screen is locked")
                    .on_toggle(|v| Message::Toggle(SettingId::PauseScreenLock, v))
                    .into(),
            ),
            row(
                SettingId::PauseBatterySaver,
                toggler(settings.pause_on_battery_saver)
                    .label("Pause on battery saver")
                    .on_toggle(|v| Message::Toggle(SettingId::PauseBatterySaver, v))
                    .into(),
            ),
            row(
                SettingId::PauseWindowCovering,
                toggler(settings.pause_on_window_covering_desktop)
                    .label("Pause when a window covers the desktop")
                    .on_toggle(|v| Message::Toggle(SettingId::PauseWindowCovering, v))
                    .into(),
            ),
            section_title("Storage", t),
            row(
                SettingId::ImportMode,
                column![
                    text("Wallpaper import mode"),
                    pick_list(IMPORT_MODE_OPTIONS, Some(settings.import_mode), Message::ImportModeSelected),
                ]
                .spacing(6)
                .into(),
            ),
            section_title("Appearance", t),
            row(
                SettingId::Theme,
                column![text("Theme"), pick_list(THEME_OPTIONS, Some(settings.theme), Message::ThemeSelected),].spacing(6).into(),
            ),
            section_title("Playback", t),
            row(
                SettingId::Muted,
                toggler(settings.muted).label("Mute wallpaper audio").on_toggle(|v| Message::Toggle(SettingId::Muted, v)).into(),
            ),
        ]
        .spacing(12)
        .into()
    }
}

fn section_title<'a>(label: &'static str, t: theme::Tokens) -> Element<'a, Message> {
    text(label).font(theme::BODY_FONT).size(13).color(t.muted_foreground).into()
}

/// Same card treatment as the Library page — surface background, thin
/// border, rounded corners — so every settings row reads as a distinct
/// element instead of bare text floating on the page background.
fn card<'a>(content: Element<'a, Message>, t: theme::Tokens) -> Element<'a, Message> {
    container(content)
        .padding(12)
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(t.surface)),
            border: Border { color: t.border, width: 1.0, radius: 10.0.into() },
            ..container::Style::default()
        })
        .into()
}
