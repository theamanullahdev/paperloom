//! The tray widget: a small themed window (`paperloom-gui --widget`) that
//! the tray icon opens on a right (or middle) click, in place of a plain
//! drop-down menu. Now playing, pause/resume, stop, shuffle, a few quick
//! switches, what the wallpaper costs right now, and shortcuts into the app.
//!
//! It follows Paperloom's theme setting, and "System" follows the desktop's
//! own light/dark preference. It closes on Esc, when it loses focus, or
//! when the tray icon is clicked again.

use std::time::Duration;

use iced::futures::StreamExt;
use iced::widget::{button, column, container, image, row, svg, text, toggler, Space};
use iced::{Alignment, Background, Border, Element, Length, Point, Shadow, Size, Subscription, Task, Theme};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

use paperloom_common::ipc::{self, Command};
use paperloom_common::{EngineState, Library, Settings};

use crate::{icons, theme};

const WIDTH: f32 = 360.0;
const HEIGHT: f32 = 610.0;

pub fn run(at: Option<(i32, i32)>) -> iced::Result {
    iced::application(Widget::new, Widget::update, Widget::view)
        .title("Paperloom")
        .theme(Widget::theme)
        .subscription(Widget::subscription)
        .default_font(theme::BODY_FONT)
        .font(include_bytes!("../assets/fonts/IBMPlexSans.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/BricolageGrotesque.ttf").as_slice())
        .window(iced::window::Settings {
            size: Size::new(WIDTH, HEIGHT),
            position: iced::window::Position::Specific(place(at)),
            decorations: false,
            resizable: false,
            level: iced::window::Level::AlwaysOnTop,
            icon: iced::window::icon::from_file_data(include_bytes!("../assets/icon-256.png"), None).ok(),
            ..Default::default()
        })
        .run()
}

/// Near where the tray was clicked (above it when the panel is at the
/// bottom, below it when it's at the top), kept on screen.
fn place(at: Option<(i32, i32)>) -> Point {
    let (screen_w, screen_h) = paperloom_common::primary_size().map_or((1920.0, 1080.0), |(w, h)| (w as f32, h as f32));
    let (x, y) = match at {
        Some((x, y)) => {
            let (x, y) = (x as f32, y as f32);
            (x - WIDTH / 2.0, if y > screen_h / 2.0 { y - HEIGHT - 14.0 } else { y + 14.0 })
        }
        None => (screen_w - WIDTH - 14.0, screen_h - HEIGHT - 56.0),
    };
    Point::new(x.clamp(8.0, (screen_w - WIDTH - 8.0).max(8.0)), y.clamp(8.0, (screen_h - HEIGHT - 8.0).max(8.0)))
}

struct Widget {
    settings: Settings,
    system_theme: iced::theme::Mode,
    state: EngineState,
    library: Library,
    engine_running: bool,
    system: System,
    engine_cpu: f32,
    engine_memory: u64,
    cores: f32,
    /// Becomes true once the window has really been focused; only losing
    /// focus after that closes it (a window manager that never focuses it
    /// must not make it close itself).
    was_focused: bool,
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
    SystemTheme(iced::theme::Mode),
    TogglePause,
    Stop,
    Shuffle,
    Muted(bool),
    PauseFullscreen(bool),
    PauseBattery(bool),
    OpenApp,
    Open(&'static str),
    Close,
    FocusGained,
    FocusLost,
}

impl Widget {
    fn new() -> (Self, Task<Message>) {
        let mut widget = Self {
            settings: paperloom_common::load(),
            system_theme: iced::theme::Mode::None,
            state: EngineState::load(),
            library: Library::load(),
            engine_running: false,
            system: System::new(),
            engine_cpu: 0.0,
            engine_memory: 0,
            cores: std::thread::available_parallelism().map_or(1.0, |n| n.get() as f32),
            was_focused: false,
        };
        widget.refresh();
        let focus = iced::window::oldest().and_then(iced::window::gain_focus);
        (widget, Task::batch([iced::system::theme().map(Message::SystemTheme), focus]))
    }

    fn theme(&self) -> Theme {
        theme::from_mode(theme::resolve(self.settings.theme, self.system_theme))
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick),
            iced::system::theme_changes().map(Message::SystemTheme),
            iced::event::listen_with(|event, _status, _window| match event {
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape), .. }) => Some(Message::Close),
                iced::Event::Window(iced::window::Event::Focused) => Some(Message::FocusGained),
                iced::Event::Window(iced::window::Event::Unfocused) => Some(Message::FocusLost),
                _ => None,
            }),
            // Another launch of the widget (the tray icon clicked again)
            // asks this one to close.
            Subscription::run(|| crate::instance::requests().map(|_| Message::Close)),
        ])
    }

    /// Reads the engine's published state and its own CPU/memory.
    fn refresh(&mut self) {
        self.state = EngineState::load();
        self.system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_cpu().with_memory());
        let engine = self.system.processes().values().find(|process| {
            process.thread_kind().is_none() && process.name().to_string_lossy().to_lowercase().starts_with("paperloom-engin")
        });
        match engine {
            Some(process) => {
                self.engine_running = true;
                self.engine_cpu = process.cpu_usage() / self.cores;
                self.engine_memory = process.memory();
            }
            None => {
                self.engine_running = false;
                self.engine_cpu = 0.0;
                self.engine_memory = 0;
            }
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick => self.refresh(),
            Message::SystemTheme(mode) => self.system_theme = mode,
            Message::TogglePause => {
                let command = if self.state.user_paused { Command::Resume } else { Command::Pause };
                let _ = ipc::send(&command);
                self.state.user_paused = !self.state.user_paused;
            }
            Message::Stop => {
                let _ = ipc::send(&Command::Stop);
                self.engine_running = false;
                self.state.current_wallpaper = None;
            }
            Message::Shuffle => self.shuffle(),
            Message::Muted(muted) => {
                self.settings.muted = muted;
                let _ = paperloom_common::save(&self.settings);
                let _ = ipc::send(&Command::Mute(muted));
            }
            Message::PauseFullscreen(value) => {
                self.settings.pause_on_fullscreen_app = value;
                let _ = paperloom_common::save(&self.settings);
            }
            Message::PauseBattery(value) => {
                self.settings.pause_on_low_battery = value;
                let _ = paperloom_common::save(&self.settings);
            }
            Message::OpenApp => {
                let _ = paperloom_common::process::command("paperloom-gui").spawn();
                return iced::exit();
            }
            Message::Open(page) => {
                let mut command = paperloom_common::process::command("paperloom-gui");
                match page {
                    "tray" => command.arg("--tray-settings"),
                    other => command.args(["--page", other]),
                };
                let _ = command.spawn();
                return iced::exit();
            }
            Message::Close => return iced::exit(),
            Message::FocusGained => self.was_focused = true,
            Message::FocusLost => {
                if self.was_focused {
                    return iced::exit();
                }
            }
        }
        Task::none()
    }

    /// Plays a random visible wallpaper other than the current one.
    fn shuffle(&mut self) {
        let candidates: Vec<_> = self
            .library
            .entries
            .iter()
            .filter(|entry| !entry.hidden && Some(&entry.path) != self.state.current_wallpaper.as_ref() && entry.path.is_file())
            .collect();
        if candidates.is_empty() {
            return;
        }
        let pick = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.subsec_nanos() as usize) % candidates.len();
        let path = candidates[pick].path.clone();
        if crate::engine_client::play(&path).is_ok() {
            self.state.current_wallpaper = Some(path);
            self.state.user_paused = false;
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let t = theme::tokens(theme::resolve(self.settings.theme, self.system_theme));

        let entry = self.state.current_wallpaper.as_ref().and_then(|path| self.library.entries.iter().find(|entry| &entry.path == path));
        let running = self.engine_running && self.state.current_wallpaper.is_some();

        // --- Header
        let header = row![
            svg(icons::film()).width(20).height(20).style(move |_theme, _status| svg::Style { color: Some(t.primary) }),
            text("Paperloom").font(theme::DISPLAY_FONT).size(18).color(t.primary),
            Space::new().width(Length::Fill),
            button(text("✕").size(14).color(t.muted_foreground)).padding([2, 8]).style(button::text).on_press(Message::Close),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        // --- Now playing: poster, name, state
        let poster: Element<'_, Message> = match entry.and_then(|entry| entry.thumbnail.as_ref()) {
            Some(path) => image(image::Handle::from_path(path)).content_fit(iced::ContentFit::Cover).width(Length::Fill).height(Length::Fixed(150.0)).into(),
            None => container(svg(icons::film()).width(34).height(34).style(move |_theme, _status| svg::Style { color: Some(t.muted_foreground) }))
                .center(Length::Fill)
                .width(Length::Fill)
                .height(Length::Fixed(150.0))
                .style(move |_theme| container::Style { background: Some(Background::Color(t.muted)), ..container::Style::default() })
                .into(),
        };
        let name = match (entry, &self.state.current_wallpaper) {
            (Some(entry), _) => entry.display_name.clone(),
            (None, Some(path)) => path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default(),
            _ => "Nothing playing".to_string(),
        };
        let (status, status_color) = if !running {
            ("No wallpaper running", t.muted_foreground)
        } else if self.state.user_paused {
            ("Paused by you", t.primary_hover)
        } else if self.state.auto_paused {
            ("Paused automatically", t.primary_hover)
        } else {
            ("Playing", t.success)
        };
        let now_playing = container(
            column![
                container(poster).clip(true).style(move |_theme| container::Style { border: Border { radius: 10.0.into(), ..Border::default() }, ..container::Style::default() }),
                text(name).size(15).color(t.foreground),
                text(status).size(12).color(status_color),
            ]
            .spacing(8),
        );

        // --- Controls
        let control = |label: &'static str, message: Message, primary: bool, enabled: bool| {
            let mut control = button(crate::ui::label(label).size(13).center())
                .padding([9, 10])
                .width(Length::Fill)
                .style(if primary { button::primary } else { button::secondary });
            if enabled {
                control = control.on_press(message);
            }
            control
        };
        let controls = row![
            control(if self.state.user_paused { "Resume" } else { "Pause" }, Message::TogglePause, true, running),
            control("Stop", Message::Stop, false, running),
            control("Shuffle", Message::Shuffle, false, !self.library.entries.is_empty()),
        ]
        .spacing(8);

        // --- Quick switches
        let switches = column![
            toggler(self.settings.muted).label("Mute audio").on_toggle(Message::Muted).size(18),
            toggler(self.settings.pause_on_fullscreen_app).label("Pause for fullscreen apps").on_toggle(Message::PauseFullscreen).size(18),
            toggler(self.settings.pause_on_low_battery).label("Pause on low battery").on_toggle(Message::PauseBattery).size(18),
        ]
        .spacing(10);

        // --- What it costs right now
        let cost = if self.engine_running {
            format!("Wallpaper using {:.1}% CPU · {}", self.engine_cpu, megabytes(self.engine_memory))
        } else {
            "Wallpaper not running — using nothing".to_string()
        };

        // --- Shortcuts
        let link = |label: &'static str, message: Message| button(crate::ui::label(label).size(13)).padding([6, 10]).style(button::text).on_press(message);
        let shortcuts = row![link("Library", Message::OpenApp), link("Usage", Message::Open("usage")), link("Settings", Message::Open("settings")), link("Tray icon", Message::Open("tray"))]
            .spacing(2)
            .wrap();

        let open_app = button(crate::ui::label("Open Paperloom").size(14).center())
            .padding([10, 12])
            .width(Length::Fill)
            .style(button::primary)
            .on_press(Message::OpenApp);

        let body = column![
            header,
            now_playing,
            controls,
            section("Quick switches", switches.into(), t),
            text(cost).size(12).color(t.muted_foreground),
            shortcuts,
            Space::new().height(Length::Fill),
            open_app,
        ]
        .spacing(12)
        .padding(16);

        container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(Background::Color(t.background)),
                border: Border { color: t.border, width: 1.0, radius: 14.0.into() },
                shadow: Shadow::default(),
                ..container::Style::default()
            })
            .into()
    }
}

fn section<'a>(title: &'static str, content: Element<'a, Message>, t: theme::Tokens) -> Element<'a, Message> {
    container(column![text(title).size(12).color(t.muted_foreground), content].spacing(10))
        .padding(12)
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(t.surface)),
            border: Border { color: t.border, width: 1.0, radius: 10.0.into() },
            ..container::Style::default()
        })
        .into()
}

fn megabytes(bytes: u64) -> String {
    format!("{:.0} MB", bytes as f64 / 1_048_576.0)
}

