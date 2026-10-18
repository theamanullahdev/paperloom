mod registry;

use std::time::{Duration, Instant};

use iced::widget::{button, column, container, pick_list, row, slider, text, toggler, Space};
use iced::{Animation, Background, Border, Element, Length, Subscription, Task};

use paperloom_common::{ImportMode, Settings, ThemeMode, TraySize, TrayStyle};
pub use registry::{matches, Entry, SettingId, ENTRIES};

use crate::theme;

const THEME_OPTIONS: &[ThemeMode] = &[ThemeMode::System, ThemeMode::Light, ThemeMode::Dark];
const TRAY_STYLE_OPTIONS: &[TrayStyle] = &[TrayStyle::Auto, TrayStyle::Colored, TrayStyle::White, TrayStyle::Black];
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
    stats: Option<Stats>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            highlighted: None,
            highlight_fade: Animation::new(false).quick(),
            autostart: paperloom_common::autostart::is_enabled(),
            autostart_error: None,
            stats: None,
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

    /// Recounts the overview tiles and storage chart (on opening the page —
    /// it walks a few folders, so not on every redraw).
    pub fn refresh(&mut self, library: &paperloom_common::Library) {
        self.stats = Some(Stats::gather(library));
    }

    /// `update_summary`: one line from the Updates page ("Up to date",
    /// "Version 0.3.0 available", ...) shown on its entry here.
    pub fn body<'a>(&'a self, settings: &'a Settings, update_summary: String, t: theme::Tokens) -> Element<'a, Message> {
        let page = iced::widget::responsive(move |size| {
            let content = self.dashboard(settings, update_summary.clone(), size.width, t);
            crate::ui::scroll_page(content).id(SCROLL_ID).into()
        });
        page.into()
    }

    /// Overview tiles and the storage chart on top, then the settings in
    /// cards by topic — two columns when there's room.
    fn dashboard<'a>(&'a self, settings: &'a Settings, update_summary: String, width: f32, t: theme::Tokens) -> Element<'a, Message> {
        let now = Instant::now();
        // One setting inside a card; flashes when jumped to or changed.
        let item = move |id: SettingId, content: Element<'a, Message>| -> Element<'a, Message> {
            let fade = if self.highlighted == Some(id) { self.highlight_fade.interpolate(0.0_f32, 1.0_f32, now) } else { 0.0 };
            container(content)
                .padding([8, 10])
                .width(Length::Fill)
                .style(move |_theme| container::Style {
                    background: (fade > 0.0).then_some(Background::Color(iced::Color { a: fade * 0.18, ..t.primary })),
                    border: Border { color: iced::Color { a: fade, ..t.primary }, width: if fade > 0.0 { 1.0 } else { 0.0 }, radius: 8.0.into() },
                    ..container::Style::default()
                })
                .into()
        };
        let hint = move |words: &'static str| text(words).size(12).color(t.muted_foreground);

        let pausing = card(
            "Pausing",
            "Paperloom stops decoding video whenever you can't see it, so it costs next to nothing.",
            column![
                item(
                    SettingId::PauseFullscreenApp,
                    toggler(settings.pause_on_fullscreen_app).label("A fullscreen app is focused").on_toggle(|v| Message::Toggle(SettingId::PauseFullscreenApp, v)).into(),
                ),
                item(
                    SettingId::PauseWindowCovering,
                    toggler(settings.pause_on_window_covering_desktop)
                        .label("A window covers the desktop")
                        .on_toggle(|v| Message::Toggle(SettingId::PauseWindowCovering, v))
                        .into(),
                ),
                item(
                    SettingId::PauseScreenLock,
                    toggler(settings.pause_on_screen_lock).label("The screen is locked").on_toggle(|v| Message::Toggle(SettingId::PauseScreenLock, v)).into(),
                ),
                item(
                    SettingId::PauseLowBattery,
                    toggler(settings.pause_on_low_battery).label("The battery is low").on_toggle(|v| Message::Toggle(SettingId::PauseLowBattery, v)).into(),
                ),
                item(
                    SettingId::LowBatteryPercent,
                    column![
                        row![text("Low battery means").size(13), Space::new().width(Length::Fill), text(format!("{}% or less", settings.low_battery_percent)).size(13).color(t.primary)],
                        battery_gauge(settings.low_battery_percent, settings.pause_on_low_battery, t),
                        slider(5..=50, settings.low_battery_percent, Message::LowBatteryChanged).step(1u8),
                        hint("Only while unplugged."),
                    ]
                    .spacing(6)
                    .into(),
                ),
            ]
            .spacing(2)
            .into(),
            t,
        );

        let appearance = card(
            "Appearance",
            "How Paperloom's own window looks.",
            item(
                SettingId::Theme,
                column![
                    text("Theme").size(13),
                    crate::ui::segmented(
                        THEME_OPTIONS.iter().map(|mode| (mode.to_string(), settings.theme == *mode, Message::ThemeSelected(*mode))).collect(),
                        t,
                    ),
                ]
                .spacing(8)
                .into(),
            ),
            t,
        );

        let playback = card(
            "Playback",
            "Applies to the wallpaper on your desktop.",
            item(SettingId::Muted, toggler(settings.muted).label("Mute wallpaper audio").on_toggle(|v| Message::Toggle(SettingId::Muted, v)).into()),
            t,
        );

        let library = card(
            "Library",
            "What happens to a video when you import it.",
            item(
                SettingId::ImportMode,
                column![
                    text("Import mode").size(13),
                    pick_list(IMPORT_MODE_OPTIONS, Some(settings.import_mode), Message::ImportModeSelected),
                    hint(match settings.import_mode {
                        ImportMode::ReferenceInPlace => "The video stays where it is; moving or deleting it breaks the wallpaper.",
                        _ => "A copy goes into Paperloom's library folder, so the original can go anywhere.",
                    }),
                ]
                .spacing(6)
                .into(),
            ),
            t,
        );

        let content = card(
            "Online content",
            "Searching the online libraries.",
            item(
                SettingId::AllowMature,
                column![
                    toggler(settings.allow_mature).label("Allow 18+ content").on_toggle(|v| Message::Toggle(SettingId::AllowMature, v)),
                    hint("Off: 18+ searches return nothing, 18+ titles are hidden, and sources' own safe search stays on."),
                ]
                .spacing(6)
                .into(),
            ),
            t,
        );

        let startup = card(
            "Startup & tray",
            "What runs when you log in, and the icon in your panel.",
            column![
                item(SettingId::Autostart, {
                    let mut content = column![
                        toggler(self.autostart).label("Start Paperloom when I log in").on_toggle(|v| Message::Toggle(SettingId::Autostart, v)),
                        hint("Only the lightweight wallpaper engine starts, and it brings back your last wallpaper."),
                    ]
                    .spacing(6);
                    if let Some(error) = &self.autostart_error {
                        content = content.push(text(format!("Couldn't change this: {error}")).size(12).color(t.destructive));
                    }
                    content.into()
                }),
                item(
                    SettingId::TrayIcon,
                    column![
                        toggler(settings.show_tray_icon).label("Show the tray icon").on_toggle(|v| Message::Toggle(SettingId::TrayIcon, v)),
                        hint("While a wallpaper plays. Click it for pause, resume, stop and Open Paperloom."),
                    ]
                    .spacing(6)
                    .into(),
                ),
                item(
                    SettingId::TrayStyle,
                    column![
                        text("Icon style").size(13),
                        pick_list(TRAY_STYLE_OPTIONS, Some(settings.tray_style), Message::TrayStyleSelected),
                        hint("Match system theme: a one-color icon, white on dark panels and black on light ones."),
                    ]
                    .spacing(6)
                    .into(),
                ),
                item(
                    SettingId::TraySize,
                    column![
                        text("Icon size").size(13),
                        crate::ui::segmented(
                            TRAY_SIZE_OPTIONS.iter().map(|size| (format!("{size:?}"), settings.tray_size == *size, Message::TraySizeSelected(*size))).collect(),
                            t,
                        ),
                    ]
                    .spacing(6)
                    .into(),
                ),
            ]
            .spacing(2)
            .into(),
            t,
        );

        let about = card(
            "About",
            "",
            item(
                SettingId::Updates,
                row![
                    column![text(format!("Paperloom {}", env!("CARGO_PKG_VERSION"))).size(15), text(update_summary).size(12).color(t.muted_foreground)].spacing(4),
                    Space::new().width(Length::Fill),
                    button(crate::ui::label("Updates")).style(button::secondary).on_press(Message::OpenUpdates),
                ]
                .align_y(iced::Alignment::Center)
                .into(),
            ),
            t,
        );

        let cards: Element<'a, Message> = if width >= 760.0 {
            row![
                column![pausing, playback, content].spacing(16).width(Length::FillPortion(1)),
                column![appearance, library, startup, about].spacing(16).width(Length::FillPortion(1)),
            ]
            .spacing(16)
            .into()
        } else {
            column![pausing, appearance, playback, library, content, startup, about].spacing(16).into()
        };

        let mut page = column![].spacing(16);
        if let Some(stats) = &self.stats {
            page = page.push(overview(stats, t)).push(storage_chart(stats, t));
        }
        page.push(cards).into()
    }
}

/// Numbers for the overview tiles and the storage chart.
pub struct Stats {
    wallpapers: usize,
    hidden: usize,
    collections: usize,
    /// Wallpaper files missing on disk (moved or deleted elsewhere).
    missing: usize,
    copied_bytes: u64,
    linked_bytes: u64,
    preview_bytes: u64,
    cache_bytes: u64,
    update_bytes: u64,
    now_playing: Option<String>,
}

impl Stats {
    fn gather(library: &paperloom_common::Library) -> Self {
        let (mut copied_bytes, mut linked_bytes, mut missing) = (0, 0, 0);
        for entry in &library.entries {
            match entry.path.metadata() {
                Ok(meta) if entry.mode == ImportMode::ReferenceInPlace => linked_bytes += meta.len(),
                Ok(meta) => copied_bytes += meta.len(),
                Err(_) => missing += 1,
            }
        }
        let now_playing = paperloom_common::EngineState::load().current_wallpaper.map(|path| {
            library
                .entries
                .iter()
                .find(|entry| entry.path == path)
                .map(|entry| entry.display_name.clone())
                .unwrap_or_else(|| path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default())
        });
        Self {
            wallpapers: library.entries.len(),
            hidden: library.entries.iter().filter(|entry| entry.hidden).count(),
            collections: library.collections.len(),
            missing,
            copied_bytes,
            linked_bytes,
            preview_bytes: dir_size(&paperloom_common::thumbnails_dir()),
            cache_bytes: dir_size(&paperloom_common::online_cache_dir()) + dir_size(&paperloom_common::downloads_dir()),
            update_bytes: dir_size(&paperloom_common::updates_dir()),
            now_playing,
        }
    }
}

fn dir_size(dir: &std::path::Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| match entry.file_type() {
                    Ok(kind) if kind.is_dir() => dir_size(&entry.path()),
                    Ok(_) => entry.metadata().map(|meta| meta.len()).unwrap_or(0),
                    Err(_) => 0,
                })
                .sum()
        })
        .unwrap_or(0)
}

/// "1.2 GB", "340 MB", "12 KB".
fn bytes(n: u64) -> String {
    let n = n as f64;
    if n >= 1e9 {
        format!("{:.1} GB", n / 1e9)
    } else if n >= 1e6 {
        format!("{:.0} MB", n / 1e6)
    } else if n >= 1e3 {
        format!("{:.0} KB", n / 1e3)
    } else {
        format!("{n:.0} B")
    }
}

/// The row of headline numbers.
fn overview<'a>(stats: &Stats, t: theme::Tokens) -> Element<'a, Message> {
    let disk = stats.copied_bytes + stats.preview_bytes + stats.cache_bytes + stats.update_bytes;
    let tile = |label: &'static str, value: String, note: String| -> Element<'a, Message> {
        container(
            column![
                text(label).size(12).color(t.muted_foreground),
                text(value).font(theme::DISPLAY_FONT).size(24).color(t.foreground).wrapping(text::Wrapping::WordOrGlyph),
                text(note).size(12).color(t.muted_foreground),
            ]
            .spacing(4),
        )
        .padding(14)
        .width(Length::Fixed(210.0))
        .style(move |_theme| tile_style(t))
        .into()
    };
    row![
        tile(
            "Wallpapers",
            stats.wallpapers.to_string(),
            match (stats.hidden, stats.missing) {
                (0, 0) => "all visible".into(),
                (hidden, 0) => format!("{hidden} hidden"),
                (0, missing) => format!("{missing} missing on disk"),
                (hidden, missing) => format!("{hidden} hidden · {missing} missing"),
            },
        ),
        tile("Collections", stats.collections.to_string(), "linked, never copied".into()),
        tile("Disk used", bytes(disk), if stats.linked_bytes > 0 { format!("+ {} linked elsewhere", bytes(stats.linked_bytes)) } else { "by Paperloom's folders".into() }),
        tile("Now playing", stats.now_playing.clone().unwrap_or_else(|| "Nothing".into()), "on your desktop".into()),
    ]
    .spacing(12)
    .wrap()
    .vertical_spacing(12)
    .into()
}

/// Where the disk space goes: a stacked bar plus a legend.
fn storage_chart<'a>(stats: &Stats, t: theme::Tokens) -> Element<'a, Message> {
    let parts = [
        ("Wallpapers (copied)", stats.copied_bytes, t.primary),
        ("Linked videos (not in Paperloom's folder)", stats.linked_bytes, iced::Color { a: 0.45, ..t.primary }),
        ("Hover previews", stats.preview_bytes, t.success),
        ("Online cache & partial downloads", stats.cache_bytes, iced::Color::from_rgb(0.33, 0.52, 0.85)),
        ("Update installers", stats.update_bytes, t.muted_foreground),
    ];
    let total: u64 = parts.iter().map(|(_, size, _)| size).sum();

    let bar: Element<'a, Message> = if total == 0 {
        container(Space::new()).width(Length::Fill).height(14).style(move |_theme| container::Style {
            background: Some(Background::Color(t.muted)),
            border: Border { radius: 7.0.into(), ..Border::default() },
            ..container::Style::default()
        })
        .into()
    } else {
        let segments = parts.iter().filter(|(_, size, _)| *size > 0).map(|(_, size, color)| {
            // Every non-empty part stays visible, however small.
            let portion = ((*size as f64 / total as f64) * 1000.0).round().max(8.0) as u16;
            let color = *color;
            container(Space::new())
                .width(Length::FillPortion(portion))
                .height(14)
                .style(move |_theme| container::Style { background: Some(Background::Color(color)), ..container::Style::default() })
                .into()
        });
        container(row(segments).spacing(2)).clip(true).style(move |_theme| container::Style {
            border: Border { radius: 7.0.into(), ..Border::default() },
            ..container::Style::default()
        })
        .into()
    };

    let legend = row(parts.iter().map(|(label, size, color)| {
        let color = *color;
        row![
            container(Space::new()).width(10).height(10).style(move |_theme| container::Style {
                background: Some(Background::Color(color)),
                border: Border { radius: 3.0.into(), ..Border::default() },
                ..container::Style::default()
            }),
            text(format!("{label}  {}", bytes(*size))).size(12).color(t.foreground),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into()
    }))
    .spacing(18)
    .wrap()
    .vertical_spacing(8);

    container(
        column![
            row![text("Storage").font(theme::BODY_FONT).size(15).color(t.foreground), Space::new().width(Length::Fill), text(bytes(total)).size(13).color(t.muted_foreground)],
            bar,
            legend,
        ]
        .spacing(12),
    )
    .padding(16)
    .width(Length::Fill)
    .style(move |_theme| tile_style(t))
    .into()
}

/// The battery threshold drawn as a battery: the red part is where the
/// wallpaper pauses.
fn battery_gauge<'a>(percent: u8, enabled: bool, t: theme::Tokens) -> Element<'a, Message> {
    let red = if enabled { t.destructive } else { t.muted_foreground };
    let low = u16::from(percent.max(1));
    let body = row![
        container(Space::new()).width(Length::FillPortion(low)).height(Length::Fill).style(move |_theme| container::Style {
            background: Some(Background::Color(iced::Color { a: 0.75, ..red })),
            ..container::Style::default()
        }),
        container(Space::new()).width(Length::FillPortion(100 - low)).height(Length::Fill).style(move |_theme| container::Style {
            background: Some(Background::Color(iced::Color { a: 0.35, ..t.success })),
            ..container::Style::default()
        }),
    ];
    row![
        container(body).padding(2).height(18).width(Length::Fill).clip(true).style(move |_theme| container::Style {
            border: Border { color: t.muted_foreground, width: 1.5, radius: 4.0.into() },
            ..container::Style::default()
        }),
        container(Space::new()).width(4).height(8).style(move |_theme| container::Style {
            background: Some(Background::Color(t.muted_foreground)),
            border: Border { radius: 1.0.into(), ..Border::default() },
            ..container::Style::default()
        }),
    ]
    .align_y(iced::Alignment::Center)
    .into()
}

const SCROLL_ID: &str = "settings-scroll";

/// Settings roughly top to bottom, for scrolling to one.
const ORDER: &[SettingId] = &[
    SettingId::PauseFullscreenApp,
    SettingId::Theme,
    SettingId::PauseWindowCovering,
    SettingId::PauseScreenLock,
    SettingId::ImportMode,
    SettingId::PauseLowBattery,
    SettingId::LowBatteryPercent,
    SettingId::Autostart,
    SettingId::Muted,
    SettingId::TrayIcon,
    SettingId::AllowMature,
    SettingId::TrayStyle,
    SettingId::TraySize,
    SettingId::Updates,
];

/// Scrolls the page so `id` is in view (a jump from the global search or
/// the tray menu). Its rank in `ORDER` stands in for its position.
fn scroll_to(id: SettingId) -> Task<Message> {
    let index = ORDER.iter().position(|entry| *entry == id).unwrap_or(0);
    let fraction = index as f32 / (ORDER.len() - 1) as f32;
    iced::widget::operation::snap_to(SCROLL_ID, iced::widget::operation::RelativeOffset { x: None, y: Some(fraction) })
}

fn tile_style(t: theme::Tokens) -> container::Style {
    container::Style {
        background: Some(Background::Color(t.surface)),
        border: Border { color: t.border, width: 1.0, radius: 12.0.into() },
        shadow: iced::Shadow { color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.12), offset: iced::Vector::new(0.0, 2.0), blur_radius: 8.0 },
        ..container::Style::default()
    }
}

/// A topic card: title, one line on what it's for, then its settings.
fn card<'a>(title: &'static str, about: &'static str, content: Element<'a, Message>, t: theme::Tokens) -> Element<'a, Message> {
    let mut column = column![text(title).font(theme::BODY_FONT).size(16).color(t.foreground)].spacing(4);
    if !about.is_empty() {
        column = column.push(text(about).size(12).color(t.muted_foreground));
    }
    container(column.push(Space::new().height(6)).push(content)).padding(14).width(Length::Fill).style(move |_theme| tile_style(t)).into()
}
