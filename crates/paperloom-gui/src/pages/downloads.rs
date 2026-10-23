//! Downloads: every queued/active/failed download with full detail, and
//! drag-to-reorder priority. Reads the shared `paperloom_fetch::Queue`
//! directly each frame — the queue is the single source of truth.

use std::collections::HashMap;
use std::time::Instant;

use iced::widget::{button, column, container, mouse_area, progress_bar, row, svg, text, Space};
use iced::{Background, Border, Element, Length, Task};

use paperloom_fetch::{Diagnosis, Job, Queue, Status};

use super::network::Meter;

use crate::{icons, theme};

#[derive(Default)]
pub struct State {
    /// The job currently being dragged (picked up by its grip handle).
    dragging: Option<String>,
    speeds: HashMap<String, Speed>,
    /// Connection meter: speed graph and connection check.
    meter: Meter,
    /// Sites the connection check tests (set by the app from the enabled
    /// sources).
    hosts: Vec<String>,
}

struct Speed {
    bytes: u64,
    at: Instant,
    bytes_per_second: f64,
}

#[derive(Debug, Clone)]
pub enum Message {
    Back,
    DragStart(String),
    DragOver(String),
    DragEnd,
    MoveToTop(String),
    Retry(String),
    Cancel(String),
    Pause(String),
    Resume(String),
    PauseAll,
    ResumeAll,
    /// "Check connection": tests the connection only, not any download.
    CheckConnection,
    Diagnosed(Diagnosis),
}

impl State {
    pub fn is_dragging(&self) -> bool {
        self.dragging.is_some()
    }

    pub fn set_hosts(&mut self, hosts: Vec<String>) {
        self.hosts = hosts;
    }

    /// Fills the graph with recent history the first time the page opens.
    pub fn seed(&mut self, history: &std::collections::VecDeque<f32>) {
        self.meter.seed(history);
    }

    /// Runs a connection check when the page opens, if the last one is old.
    pub fn opened(&mut self) -> Task<Message> {
        if self.meter.needs_check() {
            self.meter.check(self.hosts.clone()).map(Message::Diagnosed)
        } else {
            Task::none()
        }
    }

    /// Updates the speed graph and per-job transfer speed (smoothed) from
    /// the latest snapshot. Called from the app's download tick.
    pub fn sample(&mut self, jobs: &[Job]) {
        self.meter.sample();
        let now = Instant::now();
        self.speeds.retain(|id, _| jobs.iter().any(|job| &job.id == id && job.status == Status::Downloading));

        for job in jobs.iter().filter(|job| job.status == Status::Downloading) {
            match self.speeds.get_mut(&job.id) {
                Some(speed) => {
                    let elapsed = now.duration_since(speed.at).as_secs_f64();
                    if elapsed >= 0.2 {
                        let instant = job.bytes_done.saturating_sub(speed.bytes) as f64 / elapsed;
                        speed.bytes_per_second = if speed.bytes_per_second == 0.0 { instant } else { speed.bytes_per_second * 0.6 + instant * 0.4 };
                        speed.bytes = job.bytes_done;
                        speed.at = now;
                    }
                }
                None => {
                    self.speeds.insert(job.id.clone(), Speed { bytes: job.bytes_done, at: now, bytes_per_second: 0.0 });
                }
            }
        }
    }

    /// `Message::Back` is handled by the app (it owns navigation).
    pub fn update(&mut self, queue: &Queue, message: Message) -> Task<Message> {
        match message {
            Message::Back => {}
            Message::DragStart(id) => self.dragging = Some(id),
            Message::DragOver(over) => {
                if let Some(dragged) = &self.dragging {
                    if *dragged != over {
                        let jobs = queue.snapshot();
                        if let Some(position) = jobs.iter().position(|job| job.id == over) {
                            queue.move_job(dragged, position);
                        }
                    }
                }
            }
            Message::DragEnd => self.dragging = None,
            Message::MoveToTop(id) => queue.move_job(&id, 0),
            Message::Retry(id) => queue.retry(&id),
            Message::Cancel(id) => queue.cancel(&id),
            Message::Pause(id) => queue.pause(&id),
            Message::Resume(id) => queue.resume(&id),
            Message::PauseAll => queue.pause_all(),
            Message::ResumeAll => queue.resume_all(),
            Message::CheckConnection => return self.meter.check(self.hosts.clone()).map(Message::Diagnosed),
            Message::Diagnosed(diagnosis) => self.meter.finish_check(diagnosis),
        }
        Task::none()
    }

    pub fn header<'a>(&'a self) -> Element<'a, Message> {
        button(crate::ui::label("Back to Online")).style(crate::ui::secondary).on_press(Message::Back).into()
    }

    pub fn body<'a>(&'a self, t: theme::Tokens, jobs: Vec<Job>) -> Element<'a, Message> {
        let mut list = column![self.meter.view(t, Message::CheckConnection)].spacing(14);

        if jobs.is_empty() {
            list = list.push(text("No downloads yet. Pick wallpapers in the Online tab.").color(t.muted_foreground));
        } else {
            let any_running = jobs.iter().any(|job| matches!(job.status, Status::Queued | Status::Waiting | Status::Downloading));
            let any_paused = jobs.iter().any(|job| job.status == Status::Paused);
            let mut controls = row![text("Drag the handle to change download order. The top one goes first, and waits for its connection before the next one starts.").size(12).color(t.muted_foreground)].spacing(12).align_y(iced::Alignment::Center);
            controls = controls.push(Space::new().width(Length::Fill));
            if any_running {
                controls = controls.push(button(crate::ui::label("Pause all").size(12)).style(crate::ui::secondary).padding([4, 10]).on_press(Message::PauseAll));
            }
            if any_paused {
                controls = controls.push(button(crate::ui::label("Resume all").size(12)).style(crate::ui::secondary).padding([4, 10]).on_press(Message::ResumeAll));
            }
            list = list.push(controls);
            for job in jobs {
                let speed = self.speeds.get(&job.id).map(|s| s.bytes_per_second).unwrap_or(0.0);
                list = list.push(job_row(job, speed, self.dragging.as_deref(), t));
            }
        }
        crate::ui::scroll_page(list).into()
    }
}

fn job_row<'a>(job: Job, bytes_per_second: f64, dragging: Option<&str>, t: theme::Tokens) -> Element<'a, Message> {
    let id = job.id.clone();
    let being_dragged = dragging == Some(job.id.as_str());
    let reorderable = job.status != Status::Done;

    let grip: Element<'a, Message> = if reorderable {
        mouse_area(svg(icons::grip()).width(18).height(18).style(move |_theme, _status| svg::Style { color: Some(t.muted_foreground) }))
            .on_press(Message::DragStart(id.clone()))
            .interaction(iced::mouse::Interaction::Grab)
            .into()
    } else {
        Space::new().width(18).into()
    };

    let (status_label, status_color) = match job.status {
        Status::Queued => (if job.last_error.is_some() { "Waiting for connection" } else { "Queued" }, t.muted_foreground),
        Status::Downloading => ("Downloading", t.primary),
        Status::Waiting => ("Will retry", t.primary_hover),
        Status::Done => ("Finished — adding to library", t.success),
        Status::Failed => ("Failed", t.destructive),
        Status::Paused => ("Paused", t.muted_foreground),
    };

    let fraction = match (job.bytes_total, job.status) {
        (Some(total), _) if total > 0 => (job.bytes_done as f32 / total as f32).clamp(0.0, 1.0),
        (_, Status::Done) => 1.0,
        _ => 0.0,
    };
    let bar = progress_bar(0.0..=1.0, fraction).girth(6).style(move |_theme| progress_bar::Style {
        background: Background::Color(t.muted),
        bar: Background::Color(if job.status == Status::Failed { t.destructive } else { t.primary }),
        border: Border { radius: 3.0.into(), ..Border::default() },
    });

    let mut progress_line = match job.bytes_total {
        Some(total) if total > 0 => format!("{:.0}%   {} of {}", fraction * 100.0, megabytes(job.bytes_done), megabytes(total)),
        _ => megabytes(job.bytes_done),
    };
    if job.status == Status::Downloading && bytes_per_second > 1.0 {
        progress_line.push_str(&format!("   {}/s", megabytes(bytes_per_second as u64)));
        if let Some(total) = job.bytes_total {
            let remaining = total.saturating_sub(job.bytes_done) as f64 / bytes_per_second;
            progress_line.push_str(&format!("   {} left", duration(remaining)));
        }
    }
    if let Some(seconds) = job.retry_in {
        progress_line.push_str(&format!("   retrying in {}", duration(seconds as f64)));
    }

    let detail = |label: &'static str, value: String| -> Element<'a, Message> {
        row![text(label).size(11).color(t.muted_foreground).width(Length::Fixed(86.0)), text(value).size(11).color(t.foreground)].wrap().into()
    };

    let mut details = column![
        detail("Source", job.source.clone()),
        detail("Download URL", job.url.clone()),
        detail("Saves to", job.final_path().display().to_string()),
    ]
    .spacing(2);
    if job.status != Status::Done {
        details = details.push(detail("Partial file", job.part_path().display().to_string()));
    }
    details = details.push(detail("Attempts", job.attempts.to_string()));
    if let Some(error) = &job.last_error {
        details = details.push(detail("Last message", error.clone()));
    }

    let small = |label: &'static str, message: Message| button(crate::ui::label(label).size(12)).style(crate::ui::secondary).padding([4, 10]).on_press(message);
    let mut actions = column![].spacing(6).align_x(iced::Alignment::End);
    if reorderable {
        actions = actions.push(
            button(
                row![svg(icons::move_to_top()).width(14).height(14).style(|_theme, _status| svg::Style { color: Some(iced::Color::WHITE) }), text("Top").size(12)]
                    .spacing(4)
                    .align_y(iced::Alignment::Center),
            )
            .style(crate::ui::secondary)
            .padding([4, 10])
            .on_press(Message::MoveToTop(id.clone())),
        );
    }
    if matches!(job.status, Status::Failed | Status::Waiting) {
        actions = actions.push(small("Retry now", Message::Retry(id.clone())));
    }
    match job.status {
        Status::Queued | Status::Waiting | Status::Downloading => actions = actions.push(small("Pause", Message::Pause(id.clone()))),
        Status::Paused => actions = actions.push(small("Resume", Message::Resume(id.clone()))),
        _ => {}
    }
    if job.status != Status::Done {
        actions = actions.push(small("Cancel", Message::Cancel(id.clone())));
    }

    let content = row![
        container(grip).padding([2, 0]),
        column![
            row![text(job.title.clone()).size(14).color(t.foreground), Space::new().width(Length::Fill), text(status_label).size(12).color(status_color)]
                .align_y(iced::Alignment::Center),
            bar,
            text(progress_line).size(12).color(t.muted_foreground),
            details,
        ]
        .spacing(6)
        .width(Length::Fill),
        actions,
    ]
    .spacing(12);

    mouse_area(container(content).padding(12).width(Length::Fill).style(move |_theme| container::Style {
        background: Some(Background::Color(t.surface)),
        border: Border {
            color: if being_dragged { t.primary } else { t.border },
            width: if being_dragged { 2.0 } else { 1.0 },
            radius: 10.0.into(),
        },
        ..container::Style::default()
    }))
    .on_enter(Message::DragOver(job.id))
    .into()
}

pub(crate) fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_048_576.0)
}

pub(crate) fn duration(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    match seconds {
        0..=59 => format!("{seconds}s"),
        60..=3599 => format!("{}m {}s", seconds / 60, seconds % 60),
        _ => format!("{}h {}m", seconds / 3600, seconds % 3600 / 60),
    }
}
