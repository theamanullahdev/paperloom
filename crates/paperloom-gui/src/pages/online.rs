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

use iced::widget::{button, column, container, grid, image, row, stack, svg, text, Space};
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

/// How many related keywords a search also runs (on top of the query itself).
const RELATED_SEARCHES: usize = 4;
/// Keep loading further pages (up to `MAX_AUTO_ROUNDS` times) while a search
/// shows fewer cards than this, so a narrow keyword still fills the page.
const MIN_RESULTS: usize = 24;
const MAX_AUTO_ROUNDS: u32 = 3;
/// Saved results younger than this are shown as they are, with no refetch.
const FRESH_FOR: Duration = Duration::from_secs(15 * 60);

/// One keyword being searched across every enabled source: the query
/// itself, or one of its related keywords.
struct Stream {
    query: String,
    page: u32,
    cursors: HashMap<String, Cursor>,
    has_more: bool,
    loading: bool,
}

impl Stream {
    fn new(query: String) -> Self {
        Self { query, page: 0, cursors: HashMap::new(), has_more: true, loading: false }
    }
}

pub struct State {
    queue: Queue,
    /// What the user searched for ("" = latest wallpapers).
    active_query: String,
    /// `streams[0]` is the query itself; the rest are related keywords.
    streams: Vec<Stream>,
    items: Vec<OnlineItem>,
    thumbs: HashMap<String, PathBuf>,
    failure: Option<Failure>,
    notices: Vec<String>,
    filter: QualityFilter,
    /// Bumped on every new search/refresh so a slow response for an old
    /// query can't overwrite the results of a newer one.
    generation: u64,
    loaded_once: bool,
    auto_rounds: u32,
    /// Which saved search this page's results belong to (see
    /// `paperloom_fetch::load_listing`).
    cache_key: String,
    /// The cards on screen came from the saved copy and are older than
    /// `FRESH_FOR`: replaced as soon as fresh results arrive, kept if the
    /// network never answers.
    saved_pending: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// Search from the page itself (related-keyword chips, Clear).
    Search(String),
    Reload,
    LoadMore,
    SetFilter(QualityFilter),
    Loaded { generation: u64, stream: usize, page: u32, result: Result<Page, Failure> },
    ThumbLoaded(String, Result<PathBuf, String>),
    Download(String, &'static str),
    Retry(String),
    Cancel(String),
    Recheck,
}

/// Runs blocking work (HTTP) on its own thread and awaits the result, so
/// the UI's async executor is never blocked on the network.
pub(crate) fn blocking<T: Send + 'static, E: From<String> + Send + 'static>(
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
            active_query: String::new(),
            streams: Vec::new(),
            items: Vec::new(),
            thumbs: HashMap::new(),
            failure: None,
            notices: Vec::new(),
            filter: QualityFilter::All,
            generation: 0,
            loaded_once: false,
            auto_rounds: 0,
            cache_key: String::new(),
            saved_pending: false,
        }
    }

    pub fn queue(&self) -> &Queue {
        &self.queue
    }

    fn loading(&self) -> bool {
        self.streams.iter().any(|stream| stream.loading)
    }

    fn has_more(&self) -> bool {
        self.streams.iter().any(|stream| stream.has_more)
    }

    /// While offline and on this tab, quietly re-check every few seconds
    /// and reload as soon as the connection is back.
    pub fn subscription(&self, active: bool) -> Subscription<Message> {
        if active && !self.loading() && matches!(self.failure, Some(Failure::Offline)) {
            iced::time::every(Duration::from_secs(4)).map(|_| Message::Recheck)
        } else {
            Subscription::none()
        }
    }

    /// Loads the latest wallpapers the first time the tab is opened.
    pub fn ensure_loaded(&mut self, settings: &Settings) -> Task<Message> {
        if self.loaded_once || self.loading() {
            Task::none()
        } else {
            self.search(settings, String::new())
        }
    }

    /// Starts a new search: the query itself plus a few related keywords,
    /// all at once; results are merged as each one comes back. Saved
    /// results for the same search show immediately, and the network is
    /// only asked again once they're older than `FRESH_FOR`.
    pub fn search(&mut self, settings: &Settings, query: String) -> Task<Message> {
        self.start_search(settings, query, false)
    }

    /// `force`: refetch even if the saved results are fresh (Reload,
    /// "Try again", the reconnect check).
    fn start_search(&mut self, settings: &Settings, query: String, force: bool) -> Task<Message> {
        self.active_query = query.trim().to_string();
        self.generation += 1;
        self.loaded_once = true;
        self.items.clear();
        self.notices.clear();
        self.failure = None;
        self.auto_rounds = 0;
        self.saved_pending = false;
        self.cache_key = cache_key(&self.active_query, settings);
        self.streams = std::iter::once(self.active_query.clone())
            .chain(paperloom_fetch::keywords::related_queries(&self.active_query, RELATED_SEARCHES))
            .map(Stream::new)
            .collect();

        let mut tasks = Vec::new();
        let mut fresh = false;
        if let Some(saved) = paperloom_fetch::load_listing(&self.cache_key) {
            fresh = !force && saved.age < FRESH_FOR;
            self.saved_pending = !fresh;
            for item in saved.items {
                self.push_item(item, &mut tasks);
            }
        }
        if !fresh {
            tasks.extend((0..self.streams.len()).map(|index| self.load(settings, index)));
        }
        Task::batch(tasks)
    }

    /// Adds a card (once) and makes sure its picture will appear: straight
    /// from the disk cache when it's there, otherwise fetched in the
    /// background.
    fn push_item(&mut self, item: OnlineItem, fetches: &mut Vec<Task<Message>>) {
        let key = item.key();
        if self.items.iter().any(|existing| existing.key() == key) {
            return;
        }
        if !item.thumb_url.is_empty() && !self.thumbs.contains_key(&key) {
            if let Some(path) = paperloom_fetch::cached_thumbnail(&item.thumb_url) {
                self.thumbs.insert(key.clone(), path);
            } else {
                let url = item.thumb_url.clone();
                let thumb_key = key.clone();
                fetches.push(Task::perform(
                    blocking(move || paperloom_fetch::fetch_thumbnail(&url).map_err(|err| err.to_string())),
                    move |result| Message::ThumbLoaded(thumb_key.clone(), result),
                ));
            }
        }
        self.items.push(item);
    }

    /// The next page of one stream.
    fn load(&mut self, settings: &Settings, index: usize) -> Task<Message> {
        let Some(stream) = self.streams.get_mut(index) else { return Task::none() };
        if stream.loading || !stream.has_more {
            return Task::none();
        }
        stream.loading = true;
        let generation = self.generation;
        let page = stream.page + 1;
        let query = stream.query.clone();
        let cursors = stream.cursors.clone();
        let sources = settings.sources.clone();
        let allow_mature = settings.allow_mature;

        Task::perform(
            blocking(move || paperloom_fetch::fetch_page(&sources, &query, page, &cursors, allow_mature).map_err(Failure::from)),
            move |result| Message::Loaded { generation, stream: index, page, result },
        )
    }

    fn load_more(&mut self, settings: &Settings) -> Task<Message> {
        self.failure = None;
        Task::batch((0..self.streams.len()).map(|index| self.load(settings, index)).collect::<Vec<_>>())
    }

    pub fn update(&mut self, settings: &Settings, message: Message) -> Task<Message> {
        match message {
            Message::Search(query) => return self.search(settings, query),
            Message::Reload | Message::Recheck => {
                if !self.loading() {
                    return self.start_search(settings, self.active_query.clone(), true);
                }
            }
            Message::LoadMore => {
                self.auto_rounds = MAX_AUTO_ROUNDS; // the user is in charge now
                return self.load_more(settings);
            }
            Message::SetFilter(filter) => self.filter = filter,
            Message::Loaded { generation, stream, page, result } => {
                if generation != self.generation {
                    return Task::none();
                }
                let Some(state) = self.streams.get_mut(stream) else { return Task::none() };
                state.loading = false;
                let mut fetches = Vec::new();
                match result {
                    Ok(listing) => {
                        state.page = page;
                        state.has_more = listing.has_more;
                        state.cursors = listing.cursors;
                        for notice in listing.notices {
                            if !self.notices.contains(&notice) {
                                self.notices.push(notice);
                            }
                        }
                        // First fresh results replace the (older) saved cards.
                        if self.saved_pending && !listing.items.is_empty() {
                            self.items.clear();
                            self.saved_pending = false;
                        }
                        for item in listing.items {
                            self.push_item(item, &mut fetches);
                        }
                        paperloom_fetch::save_listing(&self.cache_key, &self.items[..self.items.len().min(72)]);
                    }
                    Err(failure) => {
                        state.has_more = false;
                        // A related keyword failing isn't worth a banner;
                        // the query itself failing (or being offline) is.
                        if stream == 0 || matches!(failure, Failure::Offline) {
                            self.failure = Some(failure);
                        }
                    }
                }

                // Too few cards for this search? Quietly fetch more pages.
                if !self.loading()
                    && self.failure.is_none()
                    && self.visible().count() < MIN_RESULTS
                    && self.has_more()
                    && self.auto_rounds < MAX_AUTO_ROUNDS
                {
                    self.auto_rounds += 1;
                    fetches.push(self.load_more(settings));
                }
                return Task::batch(fetches);
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
            button(crate::ui::label(label).size(13))
                .style(if self.filter == filter { button::primary } else { button::secondary })
                .padding([6, 10])
                .on_press(Message::SetFilter(filter))
        };

        let mut header = row![chip("All", QualityFilter::All), chip("4K", QualityFilter::FourK), chip("HD", QualityFilter::HdOnly)]
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

        let mut page = column![self.search_summary(t)].spacing(12);
        for notice in &self.notices {
            page = page.push(text(notice.clone()).size(12).color(t.muted_foreground));
        }
        page = page.push(container(fluid).padding([4, 4]));

        if self.loading() {
            page = page.push(text("Loading more...").color(t.muted_foreground));
        } else if let Some(failure) = &self.failure {
            let message = match failure {
                Failure::Offline => "You're offline. Reconnect and try again.".to_string(),
                Failure::Other(err) => format!("Couldn't load more: {err}"),
            };
            page = page.push(
                row![text(message).color(t.muted_foreground), button(crate::ui::label("Try again")).style(button::secondary).on_press(Message::LoadMore)]
                    .spacing(12)
                    .align_y(iced::Alignment::Center),
            );
        } else if self.has_more() {
            page = page.push(button(crate::ui::label("Load more")).style(button::secondary).on_press(Message::LoadMore));
        }

        crate::ui::scroll_page(page).into()
    }

    /// "Results for X" plus the related keywords that were searched too
    /// (each one clickable on its own), so it's clear where results come
    /// from. Browsing (no query) shows nothing here.
    fn search_summary<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        if self.active_query.is_empty() {
            return Space::new().height(0).into();
        }
        let mut line = row![
            text(format!("Results for “{}”", self.active_query)).size(15).color(t.foreground),
            button(crate::ui::label("Clear").size(12)).style(button::text).padding([2, 6]).on_press(Message::Search(String::new())),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        if self.streams.len() > 1 {
            line = line.push(text("also searching:").size(12).color(t.muted_foreground));
            for stream in &self.streams[1..] {
                line = line.push(
                    button(crate::ui::label(stream.query.clone()).size(12))
                        .style(button::secondary)
                        .padding([2, 8])
                        .on_press(Message::Search(stream.query.clone())),
                );
            }
        }
        line.wrap().vertical_spacing(6).into()
    }

    fn empty_state<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        if let Some(Failure::Offline) = self.failure {
            return container(
                column![
                    svg(icons::wifi_off()).width(40).height(40).style(move |_theme, _status| svg::Style { color: Some(t.muted_foreground) }),
                    text("You're offline").size(18).color(t.foreground),
                    text("Connect to the internet — this reloads by itself when you're back.").color(t.muted_foreground),
                    button(crate::ui::label("Try again")).style(button::primary).on_press(Message::Reload),
                ]
                .spacing(12)
                .align_x(iced::Alignment::Center),
            )
            .center_x(Length::Fill)
            .padding(48)
            .into();
        }

        let mut content = column![self.search_summary(t)].spacing(12);
        if self.loading() {
            content = content.push(text("Searching...").color(t.muted_foreground));
        } else if let Some(Failure::Other(err)) = &self.failure {
            content = content.push(text(format!("Couldn't load wallpapers: {err}")).color(t.muted_foreground));
            content = content.push(button(crate::ui::label("Try again")).style(button::primary).on_press(Message::Reload));
        } else if self.items.is_empty() {
            content = content.push(text("No wallpapers found. Try a broader word, or one of the related searches above.").color(t.muted_foreground));
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
                    button(crate::ui::label(label).size(12).center())
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
    let small = |label: &'static str, message: Message| button(crate::ui::label(label).size(12)).style(button::text).padding([2, 6]).on_press(message);

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

/// One saved search per (words, enabled sources, mature filter).
fn cache_key(query: &str, settings: &Settings) -> String {
    let mut sources: Vec<&str> = settings.sources.iter().filter(|source| source.enabled).map(|source| source.id.as_str()).collect();
    sources.sort_unstable();
    format!("{}|{}|{}", query.to_lowercase(), sources.join(","), settings.allow_mature)
}
