mod registry;

use std::time::{Duration, Instant};

use iced::widget::{column, container, mouse_area, pick_list, scrollable, text, text_input, toggler};
use iced::{Animation, Background, Color, Element, Length, Subscription, Task};

use paperloom_common::{ImportMode, Settings, ThemeMode};
pub use registry::SettingId;
use registry::ENTRIES;

const THEME_OPTIONS: &[ThemeMode] = &[ThemeMode::System, ThemeMode::Light, ThemeMode::Dark];
const IMPORT_MODE_OPTIONS: &[ImportMode] = &[
    ImportMode::CopyAndOptimize,
    ImportMode::CopyNoReencode,
    ImportMode::ReferenceInPlace,
];

pub struct State {
    search: String,
    /// The setting a search result most recently jumped to, so the full
    /// list can call it out instead of just dumping the user back at the
    /// top of the page.
    highlighted: Option<SettingId>,
    /// Fades `highlighted`'s row in rather than popping it on instantly —
    /// Iced's own built-in animation support (`iced::Animation`), not an
    /// extra dependency.
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

    pub fn update(&mut self, settings: &mut Settings, message: Message) -> Task<Message> {
        match message {
            Message::SearchChanged(value) => self.search = value,
            Message::ResultSelected(id) => {
                self.search.clear();
                self.highlighted = Some(id);
                self.highlight_fade = Animation::new(false).quick();
                self.highlight_fade.go_mut(true, Instant::now());
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
                let _ = paperloom_common::save(settings);
            }
            Message::ImportModeSelected(mode) => {
                settings.import_mode = mode;
                let _ = paperloom_common::save(settings);
            }
            Message::ThemeSelected(theme) => {
                settings.theme = theme;
                let _ = paperloom_common::save(settings);
            }
        }
        Task::none()
    }

    pub fn view<'a>(&'a self, settings: &'a Settings) -> Element<'a, Message> {
        let search = text_input("Search settings... try \"dark mode\"", &self.search)
            .on_input(Message::SearchChanged)
            .padding(8);

        let body: Element<'a, Message> = if self.search.is_empty() {
            self.full_list(settings)
        } else {
            self.search_results()
        };

        scrollable(column![search, body].spacing(16).padding(16)).into()
    }

    fn search_results<'a>(&'a self) -> Element<'a, Message> {
        let results: Vec<_> = ENTRIES.iter().filter(|entry| registry::matches(entry, &self.search)).collect();

        if results.is_empty() {
            return text("No matching settings.").into();
        }

        let mut list = column![].spacing(4);
        for entry in results {
            let row = container(text(format!("{}  ·  {}", entry.section, entry.label)))
                .padding(10)
                .width(Length::Fill);
            list = list.push(mouse_area(row).on_press(Message::ResultSelected(entry.id)));
        }
        list.into()
    }

    fn full_list<'a>(&'a self, settings: &'a Settings) -> Element<'a, Message> {
        let now = Instant::now();
        let row = |id: SettingId, content: Element<'a, Message>| {
            let fade = if self.highlighted == Some(id) {
                self.highlight_fade.interpolate(0.0_f32, 1.0_f32, now)
            } else {
                0.0
            };
            container(content).padding(10).width(Length::Fill).style(move |_theme| {
                if fade > 0.0 {
                    container::Style {
                        background: Some(Background::Color(Color { a: fade, ..Color::from_rgb(0.26, 0.45, 0.85) })),
                        ..container::Style::default()
                    }
                } else {
                    container::Style::default()
                }
            })
        };

        column![
            text("Performance & idle behavior").size(18),
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
            text("Storage").size(18),
            row(
                SettingId::ImportMode,
                column![
                    text("Wallpaper import mode"),
                    pick_list(IMPORT_MODE_OPTIONS, Some(settings.import_mode), Message::ImportModeSelected),
                ]
                .spacing(6)
                .into(),
            ),
            text("Appearance").size(18),
            row(
                SettingId::Theme,
                column![
                    text("Theme"),
                    pick_list(THEME_OPTIONS, Some(settings.theme), Message::ThemeSelected),
                ]
                .spacing(6)
                .into(),
            ),
            text("Playback").size(18),
            row(
                SettingId::Muted,
                toggler(settings.muted)
                    .label("Mute wallpaper audio")
                    .on_toggle(|v| Message::Toggle(SettingId::Muted, v))
                    .into(),
            ),
        ]
        .spacing(10)
        .into()
    }
}
