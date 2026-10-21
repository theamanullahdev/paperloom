//! Developers (Settings → Developer mode): what the engine is doing right
//! now, a copyable diagnostics report, buttons that exercise the engine and
//! the app, the logs, the raw state files and the developer switches.
//! Everything here is read or triggered on demand; the page samples once a
//! second only while it is open.

mod actions;
mod suite;

use std::time::Duration;

use iced::widget::{button, column, container, row, scrollable, text, toggler, Space};
use iced::{Alignment, Background, Border, Element, Font, Length, Subscription, Task};

use paperloom_common::dev::DevSwitches;
use paperloom_common::{log, EngineState, Settings};

pub use actions::{Action, Folder};

/// The whole self-test as text (also behind `paperloom-gui --selftest`).
pub fn run_suite(settings: &Settings) -> String {
    suite::run(settings, Default::default())
}

use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Tests,
    Diagnostics,
    Actions,
    Logs,
    State,
    Switches,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    App,
    Engine,
    Crash,
}

impl LogKind {
    fn file(self) -> &'static str {
        match self {
            LogKind::App => "gui",
            LogKind::Engine => "engine",
            LogKind::Crash => "crash",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Switch {
    ForceSoftware,
    SafeVideo,
    DisableTray,
    VerboseLog,
    DisableAutoPause,
}

#[derive(Debug, Clone)]
pub enum Message {
    Back,
    Tab(Tab),
    Tick,
    // Tests
    RunTests,
    TestsDone(String),
    // Diagnostics
    Refresh,
    DiagnosticsReady(String),
    Copy(String),
    SaveDiagnostics,
    // Actions
    Run(Action),
    Done(String),
    OpenFolder(Folder),
    // Logs
    LogKind(LogKind),
    ClearLog,
    // Switches
    Set(Switch, bool),
}

pub struct State {
    tab: Tab,
    engine: paperloom_common::EngineInfo,
    engine_alive: bool,
    user_paused: bool,
    auto_paused: bool,
    wallpaper: Option<String>,
    diagnostics: String,
    gathering: bool,
    results: Vec<String>,
    busy: bool,
    log_kind: LogKind,
    log_text: String,
    switches: DevSwitches,
    test_progress: suite::Progress,
    test_running: bool,
    test_report: String,
    state_files: [String; 3],
}

impl State {
    pub fn new() -> Self {
        Self {
            tab: Tab::Overview,
            engine: Default::default(),
            engine_alive: false,
            user_paused: false,
            auto_paused: false,
            wallpaper: None,
            diagnostics: String::new(),
            gathering: false,
            results: Vec::new(),
            busy: false,
            log_kind: LogKind::App,
            log_text: String::new(),
            switches: DevSwitches::load(),
            test_progress: Default::default(),
            test_running: false,
            test_report: String::new(),
            state_files: Default::default(),
        }
    }

    /// Called when the page opens.
    pub fn open(&mut self) -> Task<Message> {
        self.switches = DevSwitches::load();
        self.sample();
        self.reload_log();
        self.reload_state_files();
        self.refresh()
    }

    pub fn subscription(&self, active: bool) -> Subscription<Message> {
        if active {
            iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick)
        } else {
            Subscription::none()
        }
    }

    fn sample(&mut self) {
        let state = EngineState::load();
        self.engine_alive = state.engine.pid != 0 && pid_alive(state.engine.pid);
        self.engine = state.engine;
        self.user_paused = state.user_paused;
        self.auto_paused = state.auto_paused;
        self.wallpaper = state.current_wallpaper.map(|path| path.display().to_string());
    }

    fn reload_log(&mut self) {
        self.log_text = log::tail(self.log_kind.file(), 300);
    }

    fn reload_state_files(&mut self) {
        let read = |path: std::path::PathBuf| std::fs::read_to_string(&path).unwrap_or_else(|_| format!("({} not found)", path.display()));
        // Keys never leave the machine through this page either.
        let config = read(paperloom_common::config_file()).lines().map(|line| if line.trim_start().starts_with("api_key") { "api_key = \"(hidden)\"".to_string() } else { line.to_string() }).collect::<Vec<_>>().join("\n");
        self.state_files = [config, read(paperloom_common::state_file()), read(paperloom_common::dev_file())];
    }

    fn refresh(&mut self) -> Task<Message> {
        if self.gathering {
            return Task::none();
        }
        self.gathering = true;
        Task::perform(crate::pages::online::blocking::<String, String>(|| Ok(paperloom_common::diagnostics::gather(&crate::diagnostics_extra()))), |result| {
            Message::DiagnosticsReady(result.unwrap_or_default())
        })
    }

    pub fn update(&mut self, settings: &Settings, message: Message) -> Task<Message> {
        match message {
            Message::Back => {}
            Message::Tab(tab) => {
                self.tab = tab;
                match tab {
                    Tab::Logs => self.reload_log(),
                    Tab::State => self.reload_state_files(),
                    _ => {}
                }
            }
            Message::Tick => {
                self.sample();
                if self.tab == Tab::Logs {
                    self.reload_log();
                }
            }
            Message::RunTests => {
                if self.test_running {
                    return Task::none();
                }
                self.test_running = true;
                self.test_report.clear();
                let (settings, progress) = (settings.clone(), self.test_progress.clone());
                return Task::perform(crate::pages::online::blocking::<String, String>(move || Ok(suite::run(&settings, progress))), |result| Message::TestsDone(result.unwrap_or_default()));
            }
            Message::TestsDone(report) => {
                self.test_running = false;
                let path = paperloom_common::logs_dir().join("selftest.txt");
                let _ = std::fs::create_dir_all(paperloom_common::logs_dir());
                let _ = std::fs::write(&path, &report);
                log::info(&format!("self-test finished: {}", report.lines().nth(1).unwrap_or_default()));
                self.test_report = report;
            }
            Message::Refresh => return self.refresh(),
            Message::DiagnosticsReady(text) => {
                self.gathering = false;
                self.diagnostics = text;
            }
            Message::Copy(text) => return iced::clipboard::write(text),
            Message::SaveDiagnostics => {
                let path = paperloom_common::logs_dir().join("diagnostics.txt");
                let _ = std::fs::create_dir_all(paperloom_common::logs_dir());
                self.note(match std::fs::write(&path, &self.diagnostics) {
                    Ok(()) => format!("Saved {}", path.display()),
                    Err(err) => format!("Couldn't save: {err}"),
                });
            }
            Message::Run(action) => {
                if self.busy {
                    return Task::none();
                }
                self.busy = true;
                self.tab = Tab::Actions;
                let settings = settings.clone();
                return Task::perform(crate::pages::online::blocking::<String, String>(move || Ok(actions::run(action, &settings))), |result| Message::Done(result.unwrap_or_default()));
            }
            Message::Done(line) => {
                self.busy = false;
                log::info(&format!("developer action: {}", line.lines().next().unwrap_or_default()));
                self.note(line);
                self.sample();
            }
            Message::OpenFolder(folder) => {
                let path = folder.path();
                let _ = std::fs::create_dir_all(&path);
                let _ = paperloom_fetch::update::open_url(&path.display().to_string());
            }
            Message::LogKind(kind) => {
                self.log_kind = kind;
                self.reload_log();
            }
            Message::ClearLog => {
                log::clear(self.log_kind.file());
                self.reload_log();
            }
            Message::Set(switch, value) => {
                match switch {
                    Switch::ForceSoftware => self.switches.force_software = value,
                    Switch::SafeVideo => self.switches.safe_video = value,
                    Switch::DisableTray => self.switches.disable_tray = value,
                    Switch::VerboseLog => self.switches.verbose_log = value,
                    Switch::DisableAutoPause => self.switches.disable_auto_pause = value,
                }
                let _ = self.switches.save();
                log::set_verbose(self.switches.verbose_log);
                self.reload_state_files();
            }
        }
        Task::none()
    }

    fn note(&mut self, line: String) {
        self.results.push(format!("{}  {line}", log::format_unix(log::now_unix()).split(' ').nth(1).unwrap_or_default()));
        if self.results.len() > 60 {
            self.results.remove(0);
        }
    }

    pub fn header<'a>(&'a self) -> Element<'a, Message> {
        button(crate::ui::label("Back to Settings")).style(button::secondary).on_press(Message::Back).into()
    }

    pub fn body<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let tabs = crate::ui::segmented(
            [
                (Tab::Overview, "Overview"),
                (Tab::Tests, "Run all tests"),
                (Tab::Diagnostics, "Diagnostics"),
                (Tab::Actions, "Actions"),
                (Tab::Logs, "Logs"),
                (Tab::State, "State"),
                (Tab::Switches, "Switches"),
            ]
            .into_iter()
            .map(|(tab, label)| (label.to_string(), self.tab == tab, Message::Tab(tab)))
            .collect(),
            t,
        );
        let content = match self.tab {
            Tab::Overview => self.overview(t),
            Tab::Tests => self.tests_tab(t),
            Tab::Diagnostics => self.diagnostics_tab(t),
            Tab::Actions => self.actions_tab(t),
            Tab::Logs => self.logs_tab(t),
            Tab::State => self.state_tab(t),
            Tab::Switches => self.switches_tab(t),
        };
        crate::ui::scroll_page(column![tabs, content].spacing(14)).height(Length::Fill).into()
    }

    fn overview<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let info = &self.engine;
        let decoder = match (self.engine_alive, info.hwdec.as_str()) {
            (false, _) => "—".to_string(),
            (true, "" | "no") => "software (CPU)".to_string(),
            (true, name) => format!("hardware ({name})"),
        };
        let pause = match (self.user_paused, self.auto_paused) {
            (false, false) => "playing".to_string(),
            (true, false) => "paused by you".to_string(),
            (false, true) => "paused automatically".to_string(),
            (true, true) => "paused by you and automatically".to_string(),
        };
        let uptime = if self.engine_alive && info.started > 0 { format_duration(log::now_unix().saturating_sub(info.started)) } else { "—".into() };
        card(
            column![
                heading("Engine", t),
                line("Running", if self.engine_alive { format!("yes (pid {})", info.pid) } else { "no".into() }, t),
                line("Version", if info.version.is_empty() { "—".into() } else { info.version.clone() }, t),
                line("Up for", uptime, t),
                line("Wallpaper", self.wallpaper.clone().unwrap_or_else(|| "none".into()), t),
                line("State", pause, t),
                line("Decoder", decoder, t),
                line("Codec", if info.codec.is_empty() { "—".into() } else { info.codec.clone() }, t),
                line("Video", if info.width > 0 { format!("{}×{} at {:.0} fps", info.width, info.height, info.fps) } else { "—".into() }, t),
                line("Video output", if info.video_output.is_empty() { "—".into() } else { info.video_output.clone() }, t),
                line("Dropped frames", info.dropped_frames.to_string(), t),
                line("Start failure", std::fs::read_to_string(paperloom_common::engine_log()).ok().filter(|text| !text.trim().is_empty()).map_or("none".into(), |text| text.trim().to_string()), t),
            ]
            .spacing(4)
            .into(),
            t,
        )
    }

    fn tests_tab<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let running: Vec<String> = self.test_progress.lock().map(|lines| lines.clone()).unwrap_or_default();
        let bar = row![
            button(crate::ui::label(if self.test_running { "Running…" } else { "Run all tests" })).on_press_maybe((!self.test_running).then_some(Message::RunTests)),
            button(crate::ui::label("Copy report")).style(button::secondary).on_press_maybe((!self.test_report.is_empty()).then(|| Message::Copy(self.test_report.clone()))),
            button(crate::ui::label("Open the saved file")).style(button::secondary).on_press(Message::OpenFolder(Folder::Logs)),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .wrap();
        let intro = text("Checks everything Paperloom does on this computer — files, video, the wallpaper engine, network, sources, updates — and writes one report you can copy and send. It takes about a minute, plays a short test clip as the wallpaper, and puts your wallpaper back afterwards.")
            .size(12)
            .color(t.muted_foreground);
        let body: Element<'a, Message> = if !self.test_report.is_empty() {
            let failed = self.test_report.lines().nth(1).unwrap_or_default().to_string();
            column![text(failed).size(14).color(t.foreground), mono_box(&self.test_report, 560.0, t)].spacing(8).into()
        } else if self.test_running || !running.is_empty() {
            let mut list = column![].spacing(2);
            for line in running {
                list = list.push(text(line).size(12).font(Font::MONOSPACE).color(t.foreground));
            }
            card(list.into(), t)
        } else {
            Space::new().into()
        };
        column![bar, intro, body].spacing(12).into()
    }

    fn diagnostics_tab<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let bar = row![
            button(crate::ui::label("Copy diagnostics")).on_press(Message::Copy(self.diagnostics.clone())),
            button(crate::ui::label(if self.gathering { "Gathering…" } else { "Refresh" })).style(button::secondary).on_press_maybe((!self.gathering).then_some(Message::Refresh)),
            button(crate::ui::label("Save to file")).style(button::secondary).on_press(Message::SaveDiagnostics),
            text("API keys are never included.").size(12).color(t.muted_foreground),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .wrap();
        column![bar, mono_box(if self.diagnostics.is_empty() { "Gathering…" } else { &self.diagnostics }, 520.0, t)].spacing(10).into()
    }

    fn actions_tab<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let go = |label: &'static str, action: Action| button(crate::ui::label(label).size(13)).style(button::secondary).padding([6, 10]).on_press_maybe((!self.busy).then_some(Message::Run(action)));
        let folder = |label: &'static str, folder: Folder| button(crate::ui::label(label).size(13)).style(button::secondary).padding([6, 10]).on_press(Message::OpenFolder(folder));
        let group = |title: &'static str, buttons: Vec<Element<'a, Message>>| -> Element<'a, Message> {
            card(column![heading(title, t), row(buttons).spacing(8).wrap().vertical_spacing(8)].spacing(8).into(), t)
        };

        let results: Element<'a, Message> = if self.results.is_empty() {
            text("Results appear here.").size(12).color(t.muted_foreground).into()
        } else {
            let mut list = column![].spacing(3);
            for result in self.results.iter().rev() {
                list = list.push(text(result.clone()).size(12).font(Font::MONOSPACE).color(t.foreground));
            }
            list.into()
        };

        column![
            group("Engine", vec![go("Start", Action::StartEngine).into(), go("Quit", Action::QuitEngine).into(), go("Restart", Action::RestartEngine).into(), go("Ping", Action::Ping).into()]),
            group(
                "Playback",
                vec![
                    go("Play test clip", Action::PlayTestClip).into(),
                    go("Stop", Action::StopPlayback).into(),
                    go("Pause", Action::Pause).into(),
                    go("Resume", Action::Resume).into(),
                    go("Mute", Action::Mute).into(),
                    go("Unmute", Action::Unmute).into(),
                    go(if self.busy { "Measuring…" } else { "Measure engine (20 s)" }, Action::Benchmark).into(),
                ]
            ),
            group("Failure drills", vec![go("Fake engine start failure", Action::DrillEngineFailure).into(), go("Reset start failure", Action::ClearStartFailure).into(), go("Panic on purpose", Action::DrillPanic).into()]),
            group(
                "Housekeeping",
                vec![
                    go("Clear online previews", Action::ClearOnlineCache).into(),
                    go("Clear thumbnails", Action::ClearThumbnails).into(),
                    go("Delete update files", Action::DeleteUpdateFiles).into(),
                    go("Reset settings", Action::ResetSettings).into(),
                ]
            ),
            group("Network", vec![go("Connection check", Action::ConnectionCheck).into()]),
            group(
                "Folders",
                vec![
                    folder("Data", Folder::Data).into(),
                    folder("Config", Folder::Config).into(),
                    folder("Logs", Folder::Logs).into(),
                    folder("Updates", Folder::Updates).into(),
                    folder("Library", Folder::Library).into()
                ]
            ),
            card(column![heading("Results", t), results].spacing(8).into(), t),
        ]
        .spacing(12)
        .into()
    }

    fn logs_tab<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let kinds = crate::ui::segmented(
            [(LogKind::App, "App"), (LogKind::Engine, "Engine"), (LogKind::Crash, "Crash")]
                .into_iter()
                .map(|(kind, label)| (label.to_string(), self.log_kind == kind, Message::LogKind(kind)))
                .collect(),
            t,
        );
        let bar = row![
            kinds,
            button(crate::ui::label("Copy").size(13)).style(button::secondary).padding([6, 10]).on_press(Message::Copy(self.log_text.clone())),
            button(crate::ui::label("Clear").size(13)).style(button::secondary).padding([6, 10]).on_press(Message::ClearLog),
            button(crate::ui::label("Open folder").size(13)).style(button::secondary).padding([6, 10]).on_press(Message::OpenFolder(Folder::Logs)),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .wrap();
        column![bar, mono_box(if self.log_text.is_empty() { "(empty)" } else { &self.log_text }, 520.0, t)].spacing(10).into()
    }

    fn state_tab<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        column![
            heading("config.toml (keys hidden)", t),
            mono_box(&self.state_files[0], 260.0, t),
            heading("engine-state.toml", t),
            mono_box(&self.state_files[1], 220.0, t),
            heading("dev.toml", t),
            mono_box(&self.state_files[2], 120.0, t),
        ]
        .spacing(8)
        .into()
    }

    fn switches_tab<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let switch = |label: &'static str, hint: &'static str, value: bool, which: Switch| -> Element<'a, Message> {
            column![toggler(value).label(label).on_toggle(move |v| Message::Set(which, v)), text(hint).size(12).color(t.muted_foreground)].spacing(2).into()
        };
        card(
            column![
                heading("Developer switches", t),
                text("Stored in dev.toml and applied only while Developer mode is on. The engine reads them every couple of seconds.").size(12).color(t.muted_foreground),
                switch("Force software decoding", "Ignores the hardware decoding setting.", self.switches.force_software, Switch::ForceSoftware),
                switch("Safe video output (Windows)", "Starts with Direct3D 11 and software decoding at once.", self.switches.safe_video, Switch::SafeVideo),
                switch("Disable the tray icon", "Hides the icon even if the setting is on.", self.switches.disable_tray, Switch::DisableTray),
                switch("Verbose logging", "Writes debug lines to the logs.", self.switches.verbose_log, Switch::VerboseLog),
                switch("Never pause automatically", "Fullscreen, lock, battery and covered-desktop pauses are ignored.", self.switches.disable_auto_pause, Switch::DisableAutoPause),
            ]
            .spacing(12)
            .into(),
            t,
        )
    }
}

fn pid_alive(pid: u32) -> bool {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let mut system = System::new();
    let pid = Pid::from_u32(pid);
    system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, ProcessRefreshKind::nothing());
    system.process(pid).is_some_and(|process| process.name().to_string_lossy().to_lowercase().starts_with("paperloom"))
}

fn format_duration(secs: u64) -> String {
    match secs {
        s if s < 60 => format!("{s} s"),
        s if s < 3600 => format!("{} min", s / 60),
        s => format!("{} h {} min", s / 3600, (s % 3600) / 60),
    }
}

fn heading<'a>(label: &str, t: theme::Tokens) -> Element<'a, Message> {
    text(label.to_string()).font(theme::BODY_FONT).size(15).color(t.foreground).into()
}

fn line<'a>(label: &'static str, value: String, t: theme::Tokens) -> Element<'a, Message> {
    row![text(label).size(12).color(t.muted_foreground).width(Length::Fixed(130.0)), text(value).size(12).color(t.foreground)].wrap().into()
}

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

/// Selectable-looking monospace text in a fixed-height scroll box.
fn mono_box<'a>(content: &str, height: f32, t: theme::Tokens) -> Element<'a, Message> {
    container(scrollable(text(content.to_string()).size(12).font(Font::MONOSPACE).color(t.foreground)).width(Length::Fill).height(Length::Fixed(height)))
        .padding(10)
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(t.muted)),
            border: Border { color: t.border, width: 1.0, radius: 8.0.into() },
            ..container::Style::default()
        })
        .into()
}

