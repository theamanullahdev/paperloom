//! Usage: what Paperloom is costing right now — CPU, memory, disk I/O and
//! network, for the app and for the wallpaper engine — plus the same
//! storage breakdown the Settings dashboard shows. Tiles refresh once a
//! second while this page is open; the graphs come from the small recorder
//! in `telemetry.rs`, so they are already filled on arrival.

use std::time::{Duration, Instant};

use iced::widget::{button, column, container, row, text, Space};
use iced::{Background, Border, Color, Element, Length, Subscription, Task};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use paperloom_common::Settings;

use super::charts::{line_chart, Series, Unit};
use super::settings::{bytes, storage_chart, tile_style, Stats};
use crate::telemetry::{History, WINDOW};
use crate::theme;


#[derive(Debug, Clone)]
pub enum Message {
    Back,
    Tick,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    App,
    Engine,
    Helper,
}

struct Row {
    kind: Kind,
    name: String,
    pid: u32,
    /// Percent of the whole CPU (all cores), like Task Manager.
    cpu: f32,
    memory: u64,
    read_per_second: u64,
    write_per_second: u64,
    run_time: u64,
}

pub struct State {
    system: System,
    cores: f32,
    rows: Vec<Row>,
    last_sample: Instant,
    last_network_total: u64,
    network_rate: f32,
    stats: Option<Stats>,
    /// Whole-machine numbers, shown only as a small line for context —
    /// the page is about what Paperloom itself uses.
    system_cpu: f32,
    system_memory_used: u64,
    system_memory_total: u64,
}

impl State {
    pub fn new() -> Self {
        Self {
            system: System::new(),
            cores: std::thread::available_parallelism().map_or(1.0, |n| n.get() as f32),
            rows: Vec::new(),
            last_sample: Instant::now(),
            last_network_total: paperloom_fetch::bytes_received(),
            network_rate: 0.0,
            stats: None,
            system_cpu: 0.0,
            system_memory_used: 0,
            system_memory_total: 0,
        }
    }

    /// Called when the page opens: takes a first reading (CPU needs two to
    /// mean anything) and measures the folders' sizes once.
    pub fn open(&mut self, library: &paperloom_common::Library) {
        self.stats = Some(Stats::gather(library));
        self.sample();
    }

    pub fn subscription(&self, active: bool) -> Subscription<Message> {
        if active {
            iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick)
        } else {
            Subscription::none()
        }
    }

    /// `Message::Back` is handled by the app (it owns navigation).
    pub fn update(&mut self, message: Message) -> Task<Message> {
        if let Message::Tick = message {
            self.sample();
        }
        Task::none()
    }

    fn sample(&mut self) {
        let elapsed = self.last_sample.elapsed().as_secs_f32().max(0.2);
        self.last_sample = Instant::now();

        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory().with_disk_usage(),
        );

        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.system_cpu = self.system.global_cpu_usage();
        self.system_memory_used = self.system.used_memory();
        self.system_memory_total = self.system.total_memory();

        let me = Pid::from_u32(std::process::id());
        let mut rows = Vec::new();
        for (pid, process) in self.system.processes() {
            // On Linux every thread is listed as its own "process" with the
            // same memory as its parent — counting them overstated memory
            // several times over.
            if process.thread_kind().is_some() {
                continue;
            }
            let name = process.name().to_string_lossy().to_string();
            let lower = name.to_lowercase();
            let kind = if *pid == me {
                Kind::App
            } else if lower.starts_with("paperloom-engin") {
                Kind::Engine
            } else if lower.starts_with("paperloom") || (lower.starts_with("ffmpeg") && process.parent().is_some_and(|parent| self.is_ours(parent))) {
                Kind::Helper
            } else {
                continue;
            };
            let disk = process.disk_usage();
            rows.push(Row {
                kind,
                name,
                pid: pid.as_u32(),
                cpu: process.cpu_usage() / self.cores,
                memory: process.memory(),
                read_per_second: (disk.read_bytes as f32 / elapsed) as u64,
                write_per_second: (disk.written_bytes as f32 / elapsed) as u64,
                run_time: process.run_time(),
            });
        }
        rows.sort_by_key(|row| (row.kind as u8, row.pid));

        let total = paperloom_fetch::bytes_received();
        self.network_rate = total.saturating_sub(self.last_network_total) as f32 / elapsed;
        self.last_network_total = total;

        self.rows = rows;
    }

    /// Whether `pid` is a Paperloom process (used to find ffmpeg children).
    fn is_ours(&self, pid: Pid) -> bool {
        self.system.process(pid).is_some_and(|process| process.name().to_string_lossy().to_lowercase().starts_with("paperloom"))
    }

    pub fn header<'a>(&'a self) -> Element<'a, Message> {
        button(crate::ui::label("Back to Settings")).style(crate::ui::secondary).on_press(Message::Back).into()
    }

    pub fn body<'a>(&'a self, _settings: &'a Settings, t: theme::Tokens, history: &'a History) -> Element<'a, Message> {
        let total_of = |kinds: &[Kind], value: fn(&Row) -> f32| -> f32 { self.rows.iter().filter(|row| kinds.contains(&row.kind)).map(value).sum() };
        let all = [Kind::App, Kind::Engine, Kind::Helper];
        let cpu = total_of(&all, |row| row.cpu);
        let memory = total_of(&all, |row| row.memory as f32) as u64;
        let read = total_of(&all, |row| row.read_per_second as f32) as u64;
        let write = total_of(&all, |row| row.write_per_second as f32) as u64;
        let engine = self.rows.iter().find(|row| row.kind == Kind::Engine);

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
        let tiles = row![
            tile(
                "Paperloom CPU",
                format!("{cpu:.1}%"),
                format!("app {:.1}% · wallpaper {:.1}%", total_of(&[Kind::App, Kind::Helper], |row| row.cpu), total_of(&[Kind::Engine], |row| row.cpu)),
            ),
            tile(
                "Paperloom memory",
                bytes(memory),
                match engine {
                    Some(engine) => format!("app {} · wallpaper {}", bytes(total_of(&[Kind::App, Kind::Helper], |row| row.memory as f32) as u64), bytes(engine.memory)),
                    None => "no wallpaper running".to_string(),
                },
            ),
            tile("Paperloom network", rate(self.network_rate), "downloaded by Paperloom".into()),
            tile("Paperloom disk", format!("{} read", rate(read as f32)), format!("{} write", rate(write as f32))),
            tile(
                "Wallpaper",
                if engine.is_some() { "Running".into() } else { "Not running".into() },
                engine.map_or("start one from the Library".into(), |engine| format!("up {}", uptime(engine.run_time))),
            ),
        ]
        .spacing(12)
        .wrap()
        .vertical_spacing(12);

        let app_color = t.primary;
        let engine_color = Color::from_rgb(0.33, 0.52, 0.85);
        let samples = History::series;

        let cpu_chart = graph_card(
            "CPU",
            "Paperloom only — % of the whole processor",
            vec![("app", app_color), ("wallpaper", engine_color)],
            line_chart(
                vec![Series { samples: samples(&history.cpu_app), color: app_color }, Series { samples: samples(&history.cpu_engine), color: engine_color }],
                t,
                Unit::Percent,
                5.0,
                WINDOW,
                110.0,
            ),
            t,
        );
        let memory_chart = graph_card(
            "Memory",
            "Paperloom only — memory in use",
            vec![("app", app_color), ("wallpaper", engine_color)],
            line_chart(
                vec![Series { samples: samples(&history.memory_app), color: app_color }, Series { samples: samples(&history.memory_engine), color: engine_color }],
                t,
                Unit::Bytes,
                128.0 * 1_048_576.0,
                WINDOW,
                110.0,
            ),
            t,
        );
        let network_chart = graph_card(
            "Network",
            "Paperloom only — bytes received per second",
            vec![("download", app_color)],
            line_chart(vec![Series { samples: samples(&history.network), color: app_color }], t, Unit::BytesPerSecond, 256.0 * 1024.0, WINDOW, 110.0),
            t,
        );
        let disk_chart = graph_card(
            "Disk",
            "Paperloom only — reads and writes per second",
            vec![("read", app_color), ("write", engine_color)],
            line_chart(
                vec![Series { samples: samples(&history.disk_read), color: app_color }, Series { samples: samples(&history.disk_write), color: engine_color }],
                t,
                Unit::BytesPerSecond,
                256.0 * 1024.0,
                WINDOW,
                110.0,
            ),
            t,
        );

        let table = self.process_table(t);
        let storage: Element<'a, Message> = match &self.stats {
            Some(stats) => storage_chart(stats, t),
            None => Space::new().into(),
        };

        let system_line = text(format!(
            "For context — your whole computer right now: CPU {:.0}% · memory {} of {}. Everything above is only Paperloom.",
            self.system_cpu,
            bytes(self.system_memory_used),
            bytes(self.system_memory_total),
        ))
        .size(12)
        .color(t.muted_foreground);

        crate::ui::scroll_page(column![tiles, system_line, graph_grid([cpu_chart, memory_chart, network_chart, disk_chart]), table, storage].spacing(16)).into()
    }

    fn process_table<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let cell = |content: String, width: f32, color: Color| text(content).size(12).color(color).width(Length::Fixed(width));
        let head = row![
            cell("Process".into(), 190.0, t.muted_foreground),
            cell("PID".into(), 70.0, t.muted_foreground),
            cell("CPU".into(), 70.0, t.muted_foreground),
            cell("Memory".into(), 90.0, t.muted_foreground),
            cell("Read/s".into(), 90.0, t.muted_foreground),
            cell("Write/s".into(), 90.0, t.muted_foreground),
            cell("Running".into(), 90.0, t.muted_foreground),
        ]
        .spacing(8);

        let mut table = column![text("Paperloom processes").font(theme::BODY_FONT).size(15).color(t.foreground), head].spacing(8);
        if self.rows.is_empty() {
            table = table.push(text("Reading…").size(12).color(t.muted_foreground));
        }
        for row_data in &self.rows {
            let label = match row_data.kind {
                Kind::App => format!("{} (this window)", row_data.name),
                Kind::Engine => format!("{} (wallpaper)", row_data.name),
                Kind::Helper => row_data.name.clone(),
            };
            table = table.push(
                row![
                    cell(label, 190.0, t.foreground),
                    cell(row_data.pid.to_string(), 70.0, t.foreground),
                    cell(format!("{:.1}%", row_data.cpu), 70.0, t.foreground),
                    cell(bytes(row_data.memory), 90.0, t.foreground),
                    cell(rate(row_data.read_per_second as f32), 90.0, t.foreground),
                    cell(rate(row_data.write_per_second as f32), 90.0, t.foreground),
                    cell(uptime(row_data.run_time), 90.0, t.foreground),
                ]
                .spacing(8),
            );
        }
        container(table.spacing(6)).padding(16).width(Length::Fill).style(move |_theme| tile_style(t)).into()
    }
}

fn rate(bytes_per_second: f32) -> String {
    if bytes_per_second >= 1_048_576.0 {
        format!("{:.1} MB/s", bytes_per_second / 1_048_576.0)
    } else {
        format!("{:.0} KB/s", bytes_per_second / 1024.0)
    }
}

fn uptime(seconds: u64) -> String {
    match seconds {
        0..=59 => format!("{seconds}s"),
        60..=3599 => format!("{}m", seconds / 60),
        _ => format!("{}h {}m", seconds / 3600, seconds % 3600 / 60),
    }
}

/// A graph with its title, what it measures, and a colour key.
fn graph_card<'a>(title: &'static str, about: &'static str, key: Vec<(&'static str, Color)>, chart: Element<'a, Message>, t: theme::Tokens) -> Element<'a, Message> {
    let mut legend = row![].spacing(14).align_y(iced::Alignment::Center);
    for (label, color) in key {
        legend = legend.push(
            row![
                container(Space::new()).width(10).height(10).style(move |_theme| container::Style {
                    background: Some(Background::Color(color)),
                    border: Border { radius: 3.0.into(), ..Border::default() },
                    ..container::Style::default()
                }),
                text(label).size(12).color(t.muted_foreground),
            ]
            .spacing(5)
            .align_y(iced::Alignment::Center),
        );
    }
    container(
        column![
            row![text(title).font(theme::BODY_FONT).size(15).color(t.foreground), Space::new().width(Length::Fill), legend].align_y(iced::Alignment::Center),
            text(about).size(12).color(t.muted_foreground),
            chart,
        ]
        .spacing(8),
    )
    .padding(14)
    .width(Length::Fill)
    .style(move |_theme| tile_style(t))
    .into()
}

/// Lays four graph cards out two per row.
fn graph_grid<'a>(cards: [Element<'a, Message>; 4]) -> Element<'a, Message> {
    let [a, b, c, d] = cards;
    column![row![a, b].spacing(12), row![c, d].spacing(12)].spacing(12).into()
}
