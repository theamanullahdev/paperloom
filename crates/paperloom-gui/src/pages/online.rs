//! Online library: browse/search every enabled source at once and queue
//! downloads (HD or 4K where offered).
//!
//! All network work happens off the UI thread — listings and preview
//! images on short-lived worker threads (`blocking`), downloads in
//! `paperloom_fetch::Queue`'s own persistent worker, which survives page
//! switches and app restarts. Finished downloads are handed to the library
//! by `app.rs`. Offline is detected up front (`paperloom_fetch::is_online`)
//! and reported immediately, then re-checked every few seconds.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

use iced::widget::{button, column, container, grid, image, row, scrollable, stack, svg, text, text_input, Space};
use iced::{Background, Border, Element, Length, Shadow, Subscription, Task};

use paperloom_common::Settings;
use paperloom_fetch::{Cursor, FetchError, Job, OnlineItem, Page, Queue, Status};

use crate::{icons, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityFilter {
    All,
    FourK,
    HdOnly,
}

#[derive(Debug, Clone)]
pub enum Failure {
    Offline,
    Other(String),
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Failure::Other(message)
    }
}

impl From<FetchError> for Failure {
    fn from(err: FetchError) -> Self {
        match err {
            FetchError::Offline => Failure::Offline,
            other => Failure::Other(other.to_string()),
        }
    }
}

pub struct State {
    queue: Queue,
    search: String,
    active_query: String,
    items: Vec<OnlineItem>,
    thumbs: HashMap<String, PathBuf>,
    page: u32,
    cursors: HashMap<String, Cursor>,
    has_more: bool,
    loading: bool,
    failure: Option<Failure>,
    notices: Vec<String>,
    filter: QualityFilter,
    /// Bumped on every new search/refresh so a slow response for an old
    /// query can't overwrite the results of a newer one.
    generation: u64,
    loaded_once: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    SearchChanged(String),
    SearchSubmitted,
    Reload,
    LoadMore,
    SetFilter(QualityFilter),
    Loaded { generation: u64, page: u32, result: Result<Page, Failure> },
    ThumbLoaded(String, Result<PathBuf, String>),
    Download(String, &'static str),
    Retry(String),
    Cancel(String),
    Recheck,
}

/// Runs blocking work (HTTP) on its own thread and awaits the result, so
/// the UI's async executor is never blocked on the network.
fn blocking<T: Send + 'static, E: From<String> + Send + 'static>(
    work: impl FnOnce() -> Result<T, E> + Send + 'static,
) -> impl Future<Output = Result<T, E>> {
    let (sender, receiver) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = sender.send(work());
    });
    async move { receiver.await.unwrap_or_else(|_| Err(E::from("background task stopped unexpectedly".to_string()))) }
}

impl State {
    pub fn new() -> Self {
        Self {
            queue: Queue::start(),
            search: String::new(),
            active_query: String::new(),
            items: Vec::new(),
            thumbs: HashMap::new(),
            page: 1,
            cursors: HashMap::new(),
            has_more: false,
            loading: false,
            failure: None,
            notices: Vec::new(),
            filter: QualityFilter::All,
            generation: 0,
            loaded_once: false,
        }
    }

    pub fn queue(&self) -> &Queue {
        &self.queue
    }

    /// While offline and on this tab, quietly re-check every few seconds
    /// and reload as soon as the connection is back.
    pub fn subscription(&self, active: bool) -> Subscription<Message> {
        if active && !self.loading && matches!(self.failure, Some(Failure::Offline)) {
            iced::time::every(Duration::from_secs(4)).map(|_| Message::Recheck)
        } else {
            Subscription::none()
        }
    }

    /// Loads the first page the first time the tab is opened.
    pub fn ensure_loaded(&mut self, settings: &Settings) -> Task<Message> {
        if self.loaded_once || self.loading {
            Task::none()
        } else {
            self.load(settings, 1)
        }
    }

    fn reset_results(&mut self) {
        self.items.clear();
        self.cursors.clear();
        self.notices.clear();
        self.page = 1;
        self.has_more = false;
    }

    fn load(&mut self, settings: &Settings, page: u32) -> Task<Message> {
        self.loading = true;
        self.loaded_once = true;
        self.failure = None;
        if page == 1 {
            self.generation += 1;
        }
        let generation = self.generation;
        let query = self.active_query.clone();
        let cursors = self.cursors.clone();
        let sources = settings.sources.clone();

        Task::perform(
            blocking(move || paperloom_fetch::fetch_page(&sources, &query, page, &cursors).map_err(Failure::from)),
            move |result| Message::Loaded { generation, page, result },
        )
    }

    pub fn update(&mut self, settings: &Settings, message: Message) -> Task<Message> {
        match message {
            Message::SearchChanged(value) => self.search = value,
            Message::SearchSubmitted => {
                self.active_query = self.search.trim().to_string();
                self.reset_results();
                return self.load(settings, 1);
            }
            Message::Reload | Message::Recheck => {
                if !self.loading {
                    self.reset_results();
                    return self.load(settings, 1);
                }
            }
            Message::LoadMore => {
                if self.has_more && !self.loading {
                    return self.load(settings, self.page + 1);
                }
            }
            Message::SetFilter(filter) => self.filter = filter,
            Message::Loaded { generation, page, result } => {
                if generation != self.generation {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(listing) => {
                        self.page = page;
                        self.has_more = listing.has_more;
                        self.cursors = listing.cursors;
                        self.notices = listing.notices;
                        if page == 1 {
                            self.items.clear();
                        }

                        let mut fetches = Vec::new();
                        for item in listing.items {
                            let key = item.key();
                            if self.items.iter().any(|existing| existing.key() == key) {
                                continue;
                            }
                            if !item.thumb_url.is_empty() && !self.thumbs.contains_key(&key) {
                                let url = item.thumb_url.clone();
                                fetches.push(Task::perform(
                                    blocking(move || paperloom_fetch::fetch_thumbnail(&url).map_err(|err| err.to_string())),
                                    move |result| Message::ThumbLoaded(key.clone(), result),
                                ));
                            }
                            self.items.push(item);
                        }
                        return Task::batch(fetches);
                    }
                    Err(failure) => self.failure = Some(failure),
                }
            }
            Message::ThumbLoaded(key, result) => {
                if let Ok(path) = result {
                    self.thumbs.insert(key, path);
                }
            }
            Message::Download(key, quality) => {
                if let Some(item) = self.items.iter().find(|item| item.key() == key) {
                    self.queue.add(item, quality);
                }
            }
            Message::Retry(id) => self.queue.retry(&id),
            Message::Cancel(id) => self.queue.cancel(&id),
        }
        Task::none()
    }

    pub fn header<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let active = self.queue.snapshot().iter().filter(|job| job.status != Status::Failed).count();

        let chip = |label: &'static str, filter: QualityFilter| {
            button(text(label).size(13))
                .style(if self.filter == filter { button::primary } else { button::secondary })
                .padding([6, 10])
                .on_press(Message::SetFilter(filter))
        };

        let mut header = row![
            text_input("Search all sources...", &self.search)
                .on_input(Message::SearchChanged)
                .on_submit(Message::SearchSubmitted)
                .padding(8)
                .width(Length::Fixed(240.0)),
            button(text("Search")).style(button::primary).on_press(Message::SearchSubmitted),
            chip("All", QualityFilter::All),
            chip("4K", QualityFilter::FourK),
            chip("HD", QualityFilter::HdOnly),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);

        if active > 0 {
            header = header.push(text(format!("{active} downloading")).size(13).color(t.muted_foreground));
        }
        header.into()
    }

    fn visible(&self) -> impl Iterator<Item = (usize, &OnlineItem)> {
        self.items.iter().enumerate().filter(|(_, item)| match self.filter {
            QualityFilter::All => true,
            QualityFilter::FourK => item.has_4k(),
            QualityFilter::HdOnly => !item.has_4k(),
        })
    }

    pub fn body<'a>(&'a self, t: theme::Tokens, in_library: &dyn Fn(&str) -> bool) -> Element<'a, Message> {
        let visible: Vec<(usize, &OnlineItem)> = self.visible().collect();

        if visible.is_empty() {
            return self.empty_state(t);
        }

        let jobs: HashMap<String, Job> = self.queue.snapshot().into_iter().map(|job| (job.id.clone(), job)).collect();

        let cards: Vec<Element<'a, Message>> = visible
            .into_iter()
            .map(|(_, item)| {
                let key = item.key();
                card(item, self.thumbs.get(&key), jobs.get(&key), in_library(&key), t)
            })
            .collect();

        let fluid = grid::Grid::with_children(cards)
            .fluid(super::gallery::FLUID_COLUMN_WIDTH)
            .spacing(16)
            .height(grid::aspect_ratio(super::gallery::card_ratio(), 1.0));

        let mut page = column![].spacing(12);
        for notice in &self.notices {
            page = page.push(text(notice.clone()).size(12).color(t.muted_foreground));
        }
        page = page.push(container(fluid).padding([4, 4]));

        if self.loading {
            page = page.push(text("Loading more...").color(t.muted_foreground));
        } else if let Some(failure) = &self.failure {
            let message = match failure {
                Failure::Offline => "You're offline. Reconnect and try again.".to_string(),
                Failure::Other(err) => format!("Couldn't load more: {err}"),
            };
            page = page.push(
                row![text(message).color(t.muted_foreground), button(text("Try again")).style(button::secondary).on_press(Message::LoadMore)]
                    .spacing(12)
                    .align_y(iced::Alignment::Center),
            );
        } else if self.has_more {
            page = page.push(button(text("Load more")).style(button::secondary).on_press(Message::LoadMore));
        }

        scrollable(container(page).padding([0, 4])).into()
    }

    fn empty_state<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        if let Some(Failure::Offline) = self.failure {
            return container(
                column![
                    svg(icons::wifi_off()).width(40).height(40).style(move |_theme, _status| svg::Style { color: Some(t.muted_foreground) }),
                    text("You're offline").size(18).color(t.foreground),
                    text("Connect to the internet — this reloads by itself when you're back.").color(t.muted_foreground),
                    button(text("Try again")).style(button::primary).on_press(Message::Reload),
                ]
                .spacing(12)
                .align_x(iced::Alignment::Center),
            )
            .center_x(Length::Fill)
            .padding(48)
            .into();
        }

        let mut content = column![].spacing(12);
        if self.loading {
            content = content.push(text("Searching...").color(t.muted_foreground));
        } else if let Some(Failure::Other(err)) = &self.failure {
            content = content.push(text(format!("Couldn't load wallpapers: {err}")).color(t.muted_foreground));
            content = content.push(button(text("Try again")).style(button::primary).on_press(Message::Reload));
        } else if self.items.is_empty() {
            content = content.push(text("No wallpapers found.").color(t.muted_foreground));
        } else {
            content = content.push(text("Nothing matches this quality filter.").color(t.muted_foreground));
        }
        for notice in &self.notices {
            content = content.push(text(notice.clone()).size(12).color(t.muted_foreground));
        }
        container(content).padding(24).into()
    }
}

fn card<'a>(item: &'a OnlineItem, thumb: Option<&'a PathBuf>, job: Option<&Job>, in_library: bool, t: theme::Tokens) -> Element<'a, Message> {
    let preview: Element<'a, Message> = match thumb {
        Some(path) => image(image::Handle::from_path(path)).content_fit(iced::ContentFit::Cover).into(),
        None => container(svg(icons::film()).width(32).height(32).style(move |_theme, _status| svg::Style { color: Some(t.muted_foreground) }))
            .center(Length::Fill)
            .style(move |_theme| container::Style { background: Some(Background::Color(t.muted)), ..container::Style::default() })
            .into(),
    };

    let badge_text = if item.has_4k() { format!("4K · {}", item.source_name) } else { format!("HD · {}", item.source_name) };
    let badge: Element<'a, Message> = container(container(text(badge_text).size(11).color(iced::Color::WHITE)).padding([2, 6]).style(|_theme| container::Style {
        background: Some(Background::Color(iced::Color { a: 0.6, ..iced::Color::BLACK })),
        border: Border { radius: 6.0.into(), ..Border::default() },
        ..container::Style::default()
    }))
    .padding(6)
    .into();

    let image_area = container(stack![preview, badge]).clip(true).width(Length::Fill).height(Length::Fill);

    let key = item.key();
    let action: Element<'a, Message> = if in_library {
        text("In your library").size(12).color(t.success).into()
    } else {
        match job {
            None => {
                let pick = |label: &'static str, quality: &'static str| {
                    button(text(label).size(12).center())
                        .style(button::primary)
                        .width(Length::Fill)
                        .on_press(Message::Download(key.clone(), quality))
                };
                if item.has_4k() {
                    row![pick("Download HD", "HD"), pick("Download 4K", "4K")].spacing(6).into()
                } else {
                    pick("Download", "HD").into()
                }
            }
            Some(job) => job_status(job, t),
        }
    };

    // Fixed height so a text-only status lines up with neighbouring cards
    // that show buttons.
    let action = container(action).height(Length::Fixed(27.0)).align_y(iced::alignment::Vertical::Center);
    let chrome = column![text(truncate(&item.title, 32)).size(13).color(t.foreground), action].spacing(8).padding([8, 2]);

    container(column![image_area, chrome].width(Length::Fill).height(Length::Fill))
        .style(move |_theme| container::Style {
            background: Some(Background::Color(t.surface)),
            border: Border { color: t.border, width: 1.0, radius: 12.0.into() },
            shadow: Shadow { color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.15), offset: iced::Vector::new(0.0, 2.0), blur_radius: 8.0 },
            ..container::Style::default()
        })
        .clip(true)
        .into()
}

/// One-line status for a queued job plus the buttons that apply to it. The
/// full detail lives on the Downloads page.
fn job_status<'a>(job: &Job, t: theme::Tokens) -> Element<'a, Message> {
    let small = |label: &'static str, message: Message| button(text(label).size(12)).style(button::text).padding([2, 6]).on_press(message);

    let (label, buttons): (String, Vec<Element<'a, Message>>) = match job.status {
        Status::Queued if job.last_error.is_some() => ("Waiting for connection".into(), vec![small("Cancel", Message::Cancel(job.id.clone())).into()]),
        Status::Queued => (format!("Queued ({})", job.quality), vec![small("Cancel", Message::Cancel(job.id.clone())).into()]),
        Status::Downloading => (progress_label(job), vec![small("Cancel", Message::Cancel(job.id.clone())).into()]),
        Status::Waiting => (
            format!("Retrying... (try {})", job.attempts + 1),
            vec![small("Retry now", Message::Retry(job.id.clone())).into(), small("Cancel", Message::Cancel(job.id.clone())).into()],
        ),
        Status::Done => ("Adding to library...".into(), Vec::new()),
        Status::Failed => (
            truncate(job.last_error.as_deref().unwrap_or("Download failed"), 22),
            vec![small("Retry", Message::Retry(job.id.clone())).into(), small("Cancel", Message::Cancel(job.id.clone())).into()],
        ),
    };

    let mut line = row![text(label).size(12).color(t.muted_foreground), Space::new().width(Length::Fill)].align_y(iced::Alignment::Center);
    for button in buttons {
        line = line.push(button);
    }
    line.into()
}

fn progress_label(job: &Job) -> String {
    match job.bytes_total {
        Some(total) if total > 0 => format!("{}%  {} / {}", job.bytes_done * 100 / total, megabytes(job.bytes_done), megabytes(total)),
        _ => megabytes(job.bytes_done),
    }
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_048_576.0)
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let head: String = text.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{head}…")
    }
}
