mod registry;

use std::time::{Duration, Instant};

use iced::widget::{button, column, container, pick_list, row, slider, text, toggler, Space};
use iced::{Animation, Background, Border, Element, Length, Subscription, Task};

use paperloom_common::{ImportMode, Settings, ThemeMode, TraySize, TrayStyle};
pub use registry::{matches, Entry, SettingId, ENTRIES};

use crate::theme;

const THEME_OPTIONS: &[ThemeMode] = &[ThemeMode::System, ThemeMode::Light, ThemeMode::Dark];
const TRAY_STYLE_OPTIONS: &[TrayStyle] = &[TrayStyle::Colored, TrayStyle::White, TrayStyle::Black];
const TRAY_SIZE_OPTIONS: &[TraySize] = &[TraySize::Small, TraySize::Medium, TraySize::Large];
const IMPORT_MODE_OPTIONS: &[ImportMode] = &[
    ImportMode::CopyAndOptimize,
    ImportMode::CopyNoReencode,
    ImportMode::ReferenceInPlace,
];

pub struct State {
    /// The setting a search result most recently jumped to, *or* the one
    /// just toggled — both cases reuse the same fade as feedback (jump:
    /// "here it is", toggle: "that registered"), rather than tracking two
    /// separate animation states for what's visually the same effect.
    highlighted: Option<SettingId>,
    highlight_fade: Animation<bool>,
    /// Read from the OS (its login-items entry is the source of truth, not
    /// the config file), refreshed after every change.
    autostart: bool,
    autostart_error: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            highlighted: None,
            highlight_fade: Animation::new(false).quick(),
            autostart: paperloom_common::autostart::is_enabled(),
            autostart_error: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    /// Jump to (and flash) a setting — sent by the global search.
    ResultSelected(SettingId),
    Toggle(SettingId, bool),
    ImportModeSelected(ImportMode),
    ThemeSelected(ThemeMode),
    LowBatteryChanged(u8),
    TrayStyleSelected(TrayStyle),
    TraySizeSelected(TraySize),
    /// Handled by the app (it owns navigation).
    OpenUpdates,
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
            Message::ResultSelected(id) => {
                self.flash(id);
                return scroll_to(id);
            }
            Message::Tick => {}
            Message::Toggle(id, value) => {
                match id {
                    SettingId::PauseFullscreenApp => settings.pause_on_fullscreen_app = value,
                    SettingId::PauseScreenLock => settings.pause_on_screen_lock = value,
                    SettingId::PauseLowBattery => settings.pause_on_low_battery = value,
                    SettingId::PauseWindowCovering => {
                        settings.pause_on_window_covering_desktop = value;
                    }
                    SettingId::Muted => settings.muted = value,
                    SettingId::Autostart => {
                        self.autostart_error = paperloom_common::autostart::set_enabled(value).err().map(|err| err.to_string());
                        self.autostart = paperloom_common::autostart::is_enabled();
                    }
                    SettingId::AllowMature => settings.allow_mature = value,
                    SettingId::TrayIcon => settings.show_tray_icon = value,
                    SettingId::ImportMode
                    | SettingId::Theme
                    | SettingId::LowBatteryPercent
                    | SettingId::Updates
                    | SettingId::TrayStyle
                    | SettingId::TraySize => {}
                }
                self.flash(id);
                let _ = paperloom_common::save(settings);
            }
            Message::ImportModeSelected(mode) => {
                settings.import_mode = mode;
                self.flash(SettingId::ImportMode);
                let _ = paperloom_common::save(settings);
            }
            Message::LowBatteryChanged(percent) => {
                settings.low_battery_percent = percent;
                self.flash(SettingId::LowBatteryPercent);
                let _ = paperloom_common::save(settings);
            }
            Message::OpenUpdates => {}
            Message::TrayStyleSelected(style) => {
                settings.tray_style = style;
                self.flash(SettingId::TrayStyle);
                let _ = paperloom_common::save(settings);
            }
            Message::TraySizeSelected(size) => {
                settings.tray_size = size;
                self.flash(SettingId::TraySize);
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

    /// `update_summary`: one line from the Updates page ("Up to date",
    /// "Version 0.3.0 available", ...) shown on its entry here.
    pub fn body<'a>(&'a self, settings: &'a Settings, update_summary: String, t: theme::Tokens) -> Element<'a, Message> {
        crate::ui::scroll_page(self.full_list(settings, update_summary, t)).id(SCROLL_ID).into()
    }

    fn full_list<'a>(&'a self, settings: &'a Settings, update_summary: String, t: theme::Tokens) -> Element<'a, Message> {
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
                SettingId::PauseLowBattery,
                toggler(settings.pause_on_low_battery)
                    .label("Pause on low battery")
                    .on_toggle(|v| Message::Toggle(SettingId::PauseLowBattery, v))
                    .into(),
            ),
            row(
                SettingId::LowBatteryPercent,
                column![
                    text(format!("Low battery means {}% or less, while unplugged", settings.low_battery_percent)),
                    container(slider(5..=50, settings.low_battery_percent, Message::LowBatteryChanged).step(1u8)).max_width(280.0),
                ]
                .spacing(6)
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
            section_title("Content", t),
            row(SettingId::AllowMature, {
                column![
                    toggler(settings.allow_mature)
                        .label("Allow 18+ content in online search")
                        .on_toggle(|v| Message::Toggle(SettingId::AllowMature, v)),
                    text("Off: 18+ searches return nothing, 18+ titles are hidden, and sources' own safe search stays on.")
                        .size(12)
                        .color(t.muted_foreground),
                ]
                .spacing(6)
                .into()
            }),
            section_title("Startup", t),
            row(SettingId::Autostart, {
                let mut content = column![
                    toggler(self.autostart)
                        .label("Start Paperloom when I log in")
                        .on_toggle(|v| Message::Toggle(SettingId::Autostart, v)),
                    text("Only the lightweight wallpaper engine starts, and it brings back your last wallpaper.")
                        .size(12)
                        .color(t.muted_foreground),
                ]
                .spacing(6);
                if let Some(error) = &self.autostart_error {
                    content = content.push(text(format!("Couldn't change this: {error}")).size(12).color(t.destructive));
                }
                content.into()
            }),
            row(SettingId::TrayIcon, {
                column![
                    toggler(settings.show_tray_icon)
                        .label("Show Paperloom in the system tray")
                        .on_toggle(|v| Message::Toggle(SettingId::TrayIcon, v)),
                    text("While a wallpaper plays: click to open Paperloom; right-click to pause, resume or stop the wallpaper.")
                        .size(12)
                        .color(t.muted_foreground),
                ]
                .spacing(6)
                .into()
            }),
            row(
                SettingId::TrayStyle,
                column![
                    text("Tray icon style"),
                    pick_list(TRAY_STYLE_OPTIONS, Some(settings.tray_style), Message::TrayStyleSelected),
                    text("Colored is the logo; white and black are a one-color outline that blends with dark or light panels.")
                        .size(12)
                        .color(t.muted_foreground),
                ]
                .spacing(6)
                .into(),
            ),
            row(
                SettingId::TraySize,
                column![
                    text("Tray icon size"),
                    pick_list(TRAY_SIZE_OPTIONS, Some(settings.tray_size), Message::TraySizeSelected),
                    text("How much of the panel's icon space it fills. Changes apply within a couple of seconds.").size(12).color(t.muted_foreground),
                ]
                .spacing(6)
                .into(),
            ),
            section_title("About", t),
            row(
                SettingId::Updates,
                row![
                    column![
                        text(format!("Paperloom {}", env!("CARGO_PKG_VERSION"))),
                        text(update_summary).size(12).color(t.muted_foreground),
                    ]
                    .spacing(4),
                    Space::new().width(Length::Fill),
                    button(crate::ui::label("Updates")).style(button::secondary).on_press(Message::OpenUpdates),
                ]
                .align_y(iced::Alignment::Center)
                .into(),
            ),
        ]
        .spacing(12)
        .into()
    }
}

const SCROLL_ID: &str = "settings-scroll";

/// Settings in the order `full_list` shows them, for scrolling to one.
const ORDER: &[SettingId] = &[
    SettingId::PauseFullscreenApp,
    SettingId::PauseScreenLock,
    SettingId::PauseLowBattery,
    SettingId::LowBatteryPercent,
    SettingId::PauseWindowCovering,
    SettingId::ImportMode,
    SettingId::Theme,
    SettingId::Muted,
    SettingId::AllowMature,
    SettingId::Autostart,
    SettingId::TrayIcon,
    SettingId::TrayStyle,
    SettingId::TraySize,
    SettingId::Updates,
];

/// Scrolls the page so `id` is in view (a jump from the global search or
/// the tray menu). Rows are similar heights, so its position in the list is
/// a good stand-in for its position on the page.
fn scroll_to(id: SettingId) -> Task<Message> {
    let index = ORDER.iter().position(|entry| *entry == id).unwrap_or(0);
    let fraction = index as f32 / (ORDER.len() - 1) as f32;
    iced::widget::operation::snap_to(SCROLL_ID, iced::widget::operation::RelativeOffset { x: None, y: Some(fraction) })
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
