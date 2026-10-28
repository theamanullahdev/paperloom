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
const HEIGHT: f32 = 640.0;

/// Where the widget sits and which way its arrow points.
#[derive(Clone, Copy)]
struct Placement {
    point: Point,
    /// The screen edge the tray icon is on; the arrow points that way.
    edge: Edge,
    /// Where along that edge the arrow sits, from the window's own edge.
    arrow_at: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

const ARROW: f32 = 10.0;
const GAP: f32 = 4.0;
const MARGIN: f32 = 8.0;

static PLACEMENT: std::sync::OnceLock<Placement> = std::sync::OnceLock::new();

fn placement() -> Placement {
    *PLACEMENT.get_or_init(|| Placement { point: Point::new(MARGIN, MARGIN), edge: Edge::Bottom, arrow_at: WIDTH / 2.0 })
}

fn window_size(edge: Edge) -> Size {
    match edge {
        Edge::Top | Edge::Bottom => Size::new(WIDTH, HEIGHT + ARROW),
        Edge::Left | Edge::Right => Size::new(WIDTH + ARROW, HEIGHT),
    }
}

pub fn run(at: Option<(i32, i32)>) -> iced::Result {
    let placed = place(at);
    let _ = PLACEMENT.set(placed);
    iced::application(Widget::new, Widget::update, Widget::view)
        .title("Paperloom")
        .theme(Widget::theme)
        .style(|_widget, theme| iced::theme::Style { background_color: iced::Color::TRANSPARENT, text_color: theme.palette().text })
        .subscription(Widget::subscription)
        .default_font(theme::BODY_FONT)
        .font(include_bytes!("../assets/fonts/IBMPlexSans.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/BricolageGrotesque.ttf").as_slice())
        .window(iced::window::Settings {
            size: window_size(placed.edge),
            position: iced::window::Position::Specific(placed.point),
            decorations: false,
            transparent: true,
            resizable: false,
            level: iced::window::Level::AlwaysOnTop,
            icon: iced::window::icon::from_file_data(include_bytes!("../assets/icon-256.png"), None).ok(),
            ..Default::default()
        })
        .run()
}

/// Next to the tray icon that was clicked, never over the taskbar: the
/// widget stays inside the screen's work area, on the side of the icon that
/// faces the screen's middle, with an arrow pointing back at the icon. An
/// icon in the overflow flyout sits higher than the taskbar, so the widget
/// does too.
fn place(at: Option<(i32, i32)>) -> Placement {
    let (screen_w, screen_h) = paperloom_common::primary_size().map_or((1920, 1080), |(w, h)| (w as i32, h as i32));
    let (left, top, right, bottom) = paperloom_common::work_area().unwrap_or((0, 0, screen_w, screen_h));
    let (left, top, right, bottom) = (left as f32, top as f32, right as f32, bottom as f32);
    let (screen_w, screen_h) = (screen_w as f32, screen_h as f32);

    let Some((ax, ay)) = at else {
        let edge = Edge::Bottom;
        let size = window_size(edge);
        return Placement { point: Point::new(right - size.width - MARGIN, bottom - size.height - MARGIN), edge, arrow_at: size.width - 40.0 };
    };
    let (ax, ay) = (ax as f32, ay as f32);

    // The taskbar's edge: where the work area stops short of the screen,
    // else (no taskbar to measure) the half of the screen the click was in.
    let edge = if bottom < screen_h - 1.0 {
        Edge::Bottom
    } else if top > 1.0 {
        Edge::Top
    } else if left > 1.0 {
        Edge::Left
    } else if right < screen_w - 1.0 {
        Edge::Right
    } else if ay > screen_h / 2.0 {
        Edge::Bottom
    } else {
        Edge::Top
    };

    let size = window_size(edge);
    let (x, y) = match edge {
        Edge::Bottom | Edge::Top => {
            let x = (ax - size.width / 2.0).clamp(left + MARGIN, (right - size.width - MARGIN).max(left + MARGIN));
            let y = if edge == Edge::Bottom {
                // Above the icon, but never lower than the work area's bottom.
                ay.min(bottom) - size.height - GAP
            } else {
                ay.max(top) + GAP
            };
            (x, y)
        }
        Edge::Left | Edge::Right => {
            let y = (ay - size.height / 2.0).clamp(top + MARGIN, (bottom - size.height - MARGIN).max(top + MARGIN));
            let x = if edge == Edge::Right { ax.min(right) - size.width - GAP } else { ax.max(left) + GAP };
            (x, y)
        }
    };
    let x = x.clamp(left + MARGIN, (right - size.width - MARGIN).max(left + MARGIN));
    let y = y.clamp(top + MARGIN, (bottom - size.height - MARGIN).max(top + MARGIN));
    let arrow_at = match edge {
        Edge::Bottom | Edge::Top => (ax - x).clamp(20.0, size.width - 20.0),
        Edge::Left | Edge::Right => (ay - y).clamp(20.0, size.height - 20.0),
    };
    Placement { point: Point::new(x, y), edge, arrow_at }
}

/// The little triangle that points at the tray icon.
struct Pointer {
    edge: Edge,
    at: f32,
    color: iced::Color,
}

impl<M> iced::widget::canvas::Program<M> for Pointer {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::widget::canvas::{Frame, Path};
        let mut frame = Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        let half = ARROW;
        let path = match self.edge {
            Edge::Bottom => Path::new(|b| {
                b.move_to(Point::new(self.at - half, 0.0));
                b.line_to(Point::new(self.at + half, 0.0));
                b.line_to(Point::new(self.at, h));
                b.close();
            }),
            Edge::Top => Path::new(|b| {
                b.move_to(Point::new(self.at - half, h));
                b.line_to(Point::new(self.at + half, h));
                b.line_to(Point::new(self.at, 0.0));
                b.close();
            }),
            Edge::Right => Path::new(|b| {
                b.move_to(Point::new(0.0, self.at - half));
                b.line_to(Point::new(0.0, self.at + half));
                b.line_to(Point::new(w, self.at));
                b.close();
            }),
            Edge::Left => Path::new(|b| {
                b.move_to(Point::new(w, self.at - half));
                b.line_to(Point::new(w, self.at + half));
                b.line_to(Point::new(0.0, self.at));
                b.close();
            }),
        };
        frame.fill(&path, self.color);
        vec![frame.into_geometry()]
    }
}

struct Widget {
    /// Desktop widgets are switched on.
    desktop_widgets: bool,
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
    /// The desktop's colors, used when the theme setting is "System".
    desktop: Option<theme::Tokens>,
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
    SystemTheme(iced::theme::Mode),
    TogglePause,
    Stop,
    Shuffle,
    Muted(bool),
    DesktopWidgets(bool),
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
            desktop_widgets: paperloom_common::widgets::Widgets::load().enabled,
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
            desktop: None,
        };
        if widget.settings.theme == paperloom_common::ThemeMode::System {
            widget.desktop = crate::desktop_colors::tokens(theme::tokens(paperloom_common::ThemeMode::Dark));
        }
        widget.refresh();
        let focus = iced::window::oldest().and_then(iced::window::gain_focus);
        (widget, Task::batch([iced::system::theme().map(Message::SystemTheme), focus]))
    }

    fn theme(&self) -> Theme {
        match self.desktop {
            Some(tokens) => theme::from_tokens("Desktop", tokens),
            None => theme::from_mode(theme::resolve(self.settings.theme, self.system_theme)),
        }
    }

    fn tokens(&self) -> theme::Tokens {
        self.desktop.unwrap_or_else(|| theme::tokens(theme::resolve(self.settings.theme, self.system_theme)))
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
            Message::DesktopWidgets(on) => {
                let _ = paperloom_common::widgets::set_enabled(on);
                self.desktop_widgets = paperloom_common::widgets::Widgets::load().enabled;
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
        let t = self.tokens();
        crate::ui::set_tokens(t);

        let entry = self.state.current_wallpaper.as_ref().and_then(|path| self.library.entries.iter().find(|entry| &entry.path == path));
        let running = self.engine_running && self.state.current_wallpaper.is_some();

        // --- Header
        let header = row![
            svg(icons::film()).width(20).height(20).style(move |_theme, _status| svg::Style { color: Some(t.primary) }),
            text("Paperloom").font(theme::DISPLAY_FONT).size(18).color(t.primary),
            Space::new().width(Length::Fill),
            button(text("✕").size(14).color(t.muted_foreground)).padding([2, 8]).style(crate::ui::ghost).on_press(Message::Close),
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
                container(poster).clip(true).style(move |_theme| container::Style { border: Border { radius: 6.0.into(), ..Border::default() }, ..container::Style::default() }),
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
                .style(if primary { crate::ui::primary } else { crate::ui::secondary });
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
            toggler(self.desktop_widgets).label("Desktop widgets").on_toggle(Message::DesktopWidgets).size(18),
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
        let link = |label: &'static str, message: Message| button(crate::ui::label(label).size(13)).padding([6, 10]).style(crate::ui::ghost).on_press(message);
        let shortcuts = row![link("Library", Message::OpenApp), link("Usage", Message::Open("usage")), link("Settings", Message::Open("settings")), link("Tray icon", Message::Open("tray"))]
            .spacing(2)
            .wrap();

        let open_app = button(crate::ui::label("Open Paperloom").size(14).center())
            .padding([10, 12])
            .width(Length::Fill)
            .style(crate::ui::primary)
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

        let card = container(body).width(Length::Fill).height(Length::Fill).style(move |_theme| container::Style {
            background: Some(Background::Color(t.background)),
            border: Border { color: t.border, width: 1.0, radius: 8.0.into() },
            shadow: Shadow::default(),
            ..container::Style::default()
        });

        let placed = placement();
        let pointer = |width: Length, height: Length| {
            iced::widget::canvas::Canvas::new(Pointer { edge: placed.edge, at: placed.arrow_at, color: t.background }).width(width).height(height)
        };
        match placed.edge {
            Edge::Bottom => column![card, pointer(Length::Fill, Length::Fixed(ARROW))].into(),
            Edge::Top => column![pointer(Length::Fill, Length::Fixed(ARROW)), card].into(),
            Edge::Right => row![card, pointer(Length::Fixed(ARROW), Length::Fill)].into(),
            Edge::Left => row![pointer(Length::Fixed(ARROW), Length::Fill), card].into(),
        }
    }
}

fn section<'a>(title: &'static str, content: Element<'a, Message>, t: theme::Tokens) -> Element<'a, Message> {
    container(column![text(title).size(12).color(t.muted_foreground), content].spacing(10))
        .padding(12)
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(t.surface)),
            border: Border { color: t.border, width: 1.0, radius: 6.0.into() },
            ..container::Style::default()
        })
        .into()
}

fn megabytes(bytes: u64) -> String {
    format!("{:.0} MB", bytes as f64 / 1_048_576.0)
}

