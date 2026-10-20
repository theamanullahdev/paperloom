//! The Downloads page's connection meter, in the spirit of Steam's: live
//! download speed with a scrolling graph, peak and session totals, and a
//! "Check connection" button that tests only the connection (link, internet,
//! name lookups, each source's site) — never touching the download queue.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::widget::{button, column, container, row, text, Space};
use iced::{Background, Border, Color, Element, Length, Rectangle, Renderer, Task, Theme};

use paperloom_fetch::{Diagnosis, Verdict};

use crate::theme;

/// Two minutes of history at one sample per half second.
const MAX_SAMPLES: usize = 240;
const MIN_SAMPLE_GAP: Duration = Duration::from_millis(450);
/// A connection check older than this is redone when the page opens.
const STALE_AFTER: Duration = Duration::from_secs(90);

pub struct Meter {
    /// Bytes per second, oldest first.
    samples: VecDeque<f32>,
    last_total: u64,
    last_at: Instant,
    smoothed: f32,
    peak: f32,
    session_start: u64,
    diagnosis: Option<Diagnosis>,
    checking: bool,
    checked_at: Option<Instant>,
}

impl Default for Meter {
    fn default() -> Self {
        let now_total = paperloom_fetch::bytes_received();
        Self {
            samples: VecDeque::new(),
            last_total: now_total,
            last_at: Instant::now(),
            smoothed: 0.0,
            peak: 0.0,
            session_start: now_total,
            diagnosis: None,
            checking: false,
            checked_at: None,
        }
    }
}

impl Meter {
    /// Records the current download rate (all HTTP traffic of this app).
    /// Cheap: one atomic read.
    pub fn sample(&mut self) {
        let elapsed = self.last_at.elapsed();
        if elapsed < MIN_SAMPLE_GAP {
            return;
        }
        let total = paperloom_fetch::bytes_received();
        let rate = total.saturating_sub(self.last_total) as f32 / elapsed.as_secs_f32();
        self.last_total = total;
        self.last_at = Instant::now();

        self.smoothed = self.smoothed * 0.4 + rate * 0.6;
        if self.smoothed < 1.0 {
            self.smoothed = 0.0;
        }
        self.peak = self.peak.max(self.smoothed);
        if self.samples.len() == MAX_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back(self.smoothed);
    }

    /// Starts a connection check unless one is already running. Only the
    /// connection is tested — nothing about downloads changes.
    pub fn check(&mut self, hosts: Vec<String>) -> Task<Diagnosis> {
        if self.checking {
            return Task::none();
        }
        self.checking = true;
        Task::perform(
            async move {
                let (sender, receiver) = iced::futures::channel::oneshot::channel();
                std::thread::spawn(move || {
                    let _ = sender.send(paperloom_fetch::diagnose(&hosts));
                });
                receiver.await
            },
            |result| {
                result.unwrap_or(Diagnosis { verdict: Verdict::Offline, internet_ms: None, hosts: Vec::new() })
            },
        )
    }

    pub fn finish_check(&mut self, diagnosis: Diagnosis) {
        self.diagnosis = Some(diagnosis);
        self.checking = false;
        self.checked_at = Some(Instant::now());
    }

    /// Whether opening the page should run a fresh check.
    pub fn needs_check(&self) -> bool {
        !self.checking && self.checked_at.is_none_or(|at| at.elapsed() > STALE_AFTER)
    }

    pub fn view<'a, M: Clone + 'a>(&'a self, t: theme::Tokens, on_check: M) -> Element<'a, M> {
        let (title, color, advice) = match self.diagnosis.as_ref().map(|d| d.verdict) {
            None => ("Checking your connection...", t.muted_foreground, ""),
            Some(Verdict::Good) => ("Connected", t.success, "Everything responds normally."),
            Some(Verdict::Slow) => (
                "Slow connection",
                t.primary_hover,
                "The connection is slow, so downloads and searches will take longer. Nothing is broken.",
            ),
            Some(Verdict::DnsProblem) => (
                "Name lookups are failing",
                t.primary_hover,
                "The internet is reachable, but your DNS server isn't answering reliably, so sites can look broken. Try again in a moment, or switch your DNS (for example 1.1.1.1 or 8.8.8.8) in your network settings.",
            ),
            Some(Verdict::Offline) => ("No internet", t.destructive, "A network is connected but nothing answers. Downloads wait and resume by themselves."),
            Some(Verdict::NoInterface) => ("No network", t.destructive, "Wi-Fi or the cable is off. Downloads wait and resume by themselves."),
        };

        let check_label = if self.checking { "Checking..." } else { "Check connection" };
        let check = button(crate::ui::label(check_label)).style(button::secondary).on_press_maybe((!self.checking).then_some(on_check));

        let dot = container(Space::new().width(10).height(10)).style(move |_theme| container::Style {
            background: Some(Background::Color(color)),
            border: Border { radius: 5.0.into(), ..Border::default() },
            ..container::Style::default()
        });

        let heading = row![dot, text(title).size(15).color(t.foreground), Space::new().width(Length::Fill), check]
            .spacing(8)
            .align_y(iced::Alignment::Center);

        let stat = |value: String, label: &'static str| {
            column![text(value).size(20).color(t.foreground), text(label).size(11).color(t.muted_foreground)].spacing(2)
        };
        let latency = match self.diagnosis.as_ref().and_then(|d| d.internet_ms) {
            Some(ms) => format!("{ms} ms"),
            None => "—".to_string(),
        };
        let stats = row![
            stat(rate(self.smoothed), "download speed"),
            stat(rate(self.peak), "peak"),
            stat(megabytes(paperloom_fetch::bytes_received().saturating_sub(self.session_start)), "downloaded this session"),
            stat(latency, "internet latency"),
        ]
        .spacing(28)
        .wrap();

        let chart = canvas::Canvas::new(Chart { samples: self.samples.iter().copied().collect(), t })
            .width(Length::Fill)
            .height(Length::Fixed(96.0));

        let mut panel = column![heading, stats, chart].spacing(12);

        if let Some(diagnosis) = &self.diagnosis {
            let mut hosts = column![].spacing(2);
            for check in &diagnosis.hosts {
                let detail = match (check.dns_ms, check.connect_ms) {
                    (None, _) => "name lookup failed".to_string(),
                    (Some(dns), None) => format!("name lookup {dns} ms, can't connect"),
                    (Some(dns), Some(connect)) => format!("name lookup {dns} ms · connect {connect} ms"),
                };
                let ok = check.dns_ms.is_some() && check.connect_ms.is_some();
                hosts = hosts.push(
                    row![
                        text(check.host.clone()).size(12).color(t.foreground).width(Length::Fixed(170.0)),
                        text(detail).size(12).color(if ok { t.muted_foreground } else { t.destructive }),
                    ],
                );
            }
            panel = panel.push(hosts);
        }
        if !advice.is_empty() {
            panel = panel.push(text(advice).size(12).color(t.muted_foreground));
        }

        container(panel)
            .padding(14)
            .width(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(Background::Color(t.surface)),
                border: Border { color: t.border, width: 1.0, radius: 10.0.into() },
                ..container::Style::default()
            })
            .into()
    }
}

fn rate(bytes_per_second: f32) -> String {
    let megabytes = bytes_per_second / 1_048_576.0;
    if megabytes >= 1.0 {
        format!("{megabytes:.1} MB/s")
    } else {
        format!("{:.0} KB/s", bytes_per_second / 1024.0)
    }
}

fn megabytes(bytes: u64) -> String {
    let megabytes = bytes as f64 / 1_048_576.0;
    if megabytes >= 1024.0 {
        format!("{:.2} GB", megabytes / 1024.0)
    } else {
        format!("{megabytes:.1} MB")
    }
}

/// The scrolling speed graph: filled area under a line, faint grid, scaled
/// to the busiest moment in view.
struct Chart {
    samples: Vec<f32>,
    t: theme::Tokens,
}

impl<M> canvas::Program<M> for Chart {
    type State = ();

    fn draw(&self, _state: &(), renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (width, height) = (bounds.width, bounds.height);

        frame.fill_rectangle(iced::Point::ORIGIN, bounds.size(), Color { a: 0.35, ..self.t.muted });
        for step in 1..4 {
            let y = height * step as f32 / 4.0;
            frame.stroke(&Path::line(iced::Point::new(0.0, y), iced::Point::new(width, y)), Stroke::default().with_color(self.t.border).with_width(1.0));
        }

        if self.samples.len() < 2 {
            return vec![frame.into_geometry()];
        }

        // Scale to the busiest sample in view, never below 256 KB/s so
        // idle noise doesn't fill the whole graph.
        let top = self.samples.iter().copied().fold(256.0 * 1024.0, f32::max) * 1.1;
        let x_at = |index: usize| width * index as f32 / (MAX_SAMPLES - 1) as f32 + width * (1.0 - (self.samples.len() as f32 / MAX_SAMPLES as f32));
        let y_at = |value: f32| height - (value / top).clamp(0.0, 1.0) * (height - 2.0) - 1.0;

        let line = Path::new(|builder| {
            builder.move_to(iced::Point::new(x_at(0), y_at(self.samples[0])));
            for (index, value) in self.samples.iter().enumerate().skip(1) {
                builder.line_to(iced::Point::new(x_at(index), y_at(*value)));
            }
        });
        let area = Path::new(|builder| {
            builder.move_to(iced::Point::new(x_at(0), height));
            for (index, value) in self.samples.iter().enumerate() {
                builder.line_to(iced::Point::new(x_at(index), y_at(*value)));
            }
            builder.line_to(iced::Point::new(x_at(self.samples.len() - 1), height));
            builder.close();
        });

        frame.fill(&area, Color { a: 0.22, ..self.t.primary });
        frame.stroke(&line, Stroke::default().with_color(self.t.primary).with_width(2.0));
        vec![frame.into_geometry()]
    }
}

/// Which sites to test: the ones the enabled sources actually use.
pub fn hosts_for(settings: &paperloom_common::Settings) -> Vec<String> {
    let mut hosts = Vec::new();
    for source in settings.sources.iter().filter(|source| source.enabled) {
        let host = match source.id.as_str() {
            paperloom_common::SOURCE_MOTIONBGS => "motionbgs.com".to_string(),
            paperloom_common::SOURCE_PEXELS => "api.pexels.com".to_string(),
            paperloom_common::SOURCE_PIXABAY => "pixabay.com".to_string(),
            // Built-in sites use their domain as their id.
            id if source.builtin => id.to_string(),
            _ => source.url.split("://").nth(1).and_then(|rest| rest.split(['/', '?']).next()).unwrap_or_default().to_string(),
        };
        if !host.is_empty() && !hosts.contains(&host) {
            hosts.push(host);
        }
    }
    if hosts.is_empty() {
        hosts.push("motionbgs.com".to_string());
    }
    hosts
}
