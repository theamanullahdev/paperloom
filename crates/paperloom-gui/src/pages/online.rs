//! Online library: browse/search free wallpaper sites and queue downloads.
//!
//! All network work happens off the UI thread — listings and preview
//! images on short-lived worker threads (`blocking`), downloads in
//! `paperloom_fetch::Queue`'s own persistent worker, which survives page
//! switches and app restarts. Finished downloads are handed to the library
//! by `app.rs`.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;

use iced::widget::{button, column, container, grid, image, row, scrollable, stack, svg, text, text_input, Space};
use iced::{Background, Border, Element, Length, Shadow, Task};

use paperloom_fetch::{Job, Listing, OnlineItem, Queue, Status};

use crate::{icons, theme};

pub struct State {
    queue: Queue,
    search: String,
    active_query: String,
    items: Vec<OnlineItem>,
    thumbs: HashMap<String, PathBuf>,
    page: u32,
    base: Option<String>,
    has_more: bool,
    loading: bool,
    error: Option<String>,
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
    Loaded { generation: u64, page: u32, result: Result<Listing, String> },
    ThumbLoaded(String, Result<PathBuf, String>),
    Download(usize),
    Retry(String),
    Cancel(String),
}

/// Runs blocking work (HTTP) on its own thread and awaits the result, so
/// the UI's async executor is never blocked on the network.
fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T, String> + Send + 'static) -> impl Future<Output = Result<T, String>> {
    let (sender, receiver) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = sender.send(work());
    });
    async move { receiver.await.unwrap_or_else(|_| Err("background task stopped unexpectedly".to_string())) }
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
            base: None,
            has_more: false,
            loading: false,
            error: None,
            generation: 0,
            loaded_once: false,
        }
    }

    pub fn queue(&self) -> &Queue {
        &self.queue
    }

    /// Loads the first page the first time the tab is opened.
    pub fn ensure_loaded(&mut self) -> Task<Message> {
        if self.loaded_once || self.loading {
            Task::none()
        } else {
            self.load(1)
        }
    }

    fn load(&mut self, page: u32) -> Task<Message> {
        self.loading = true;
        self.loaded_once = true;
        self.error = None;
        if page == 1 {
            self.generation += 1;
        }
        let generation = self.generation;
        let query = self.active_query.clone();
        let base = self.base.clone();

        Task::perform(
            blocking(move || {
                let sources = paperloom_fetch::sources();
                sources[0].fetch(&query, page, base.as_deref()).map_err(|err| err.to_string())
            }),
            move |result| Message::Loaded { generation, page, result },
        )
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SearchChanged(value) => self.search = value,
            Message::SearchSubmitted => {
                self.active_query = self.search.trim().to_string();
                self.items.clear();
                self.base = None;
                self.page = 1;
                self.has_more = false;
                return self.load(1);
            }
            Message::Reload => {
                self.items.clear();
                self.base = None;
                self.page = 1;
                return self.load(1);
            }
            Message::LoadMore => {
                if self.has_more && !self.loading {
                    return self.load(self.page + 1);
                }
            }
            Message::Loaded { generation, page, result } => {
                if generation != self.generation {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(listing) => {
                        self.page = page;
                        self.has_more = listing.has_more;
                        if page == 1 {
                            self.base = Some(listing.base).filter(|base| !base.is_empty());
                            self.items.clear();
                        }

                        let mut fetches = Vec::new();
                        for item in listing.items {
                            if self.items.iter().any(|existing| existing.id == item.id) {
                                continue;
                            }
                            if !self.thumbs.contains_key(&item.id) {
                                let (id, url) = (item.id.clone(), item.thumb_url.clone());
                                fetches.push(Task::perform(
                                    blocking(move || paperloom_fetch::fetch_thumbnail(&url).map_err(|err| err.to_string())),
                                    move |result| Message::ThumbLoaded(id.clone(), result),
                                ));
                            }
                            self.items.push(item);
                        }
                        return Task::batch(fetches);
                    }
                    Err(err) => self.error = Some(err),
                }
            }
            Message::ThumbLoaded(id, result) => {
                if let Ok(path) = result {
                    self.thumbs.insert(id, path);
                }
            }
            Message::Download(index) => {
                if let Some(item) = self.items.get(index) {
                    self.queue.add(item);
                }
            }
            Message::Retry(id) => self.queue.retry(&id),
            Message::Cancel(id) => self.queue.cancel(&id),
        }
        Task::none()
    }

    pub fn header<'a>(&'a self) -> Element<'a, Message> {
        let active = self.queue.snapshot().iter().filter(|job| job.status != Status::Failed).count();

        let mut header = row![
            text_input("Search online wallpapers...", &self.search)
                .on_input(Message::SearchChanged)
                .on_submit(Message::SearchSubmitted)
                .padding(8)
                .width(Length::Fixed(260.0)),
            button(text("Search")).style(button::primary).on_press(Message::SearchSubmitted),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);

        if active > 0 {
            header = header.push(text(format!("{active} downloading")).size(13));
        }
        header.into()
    }

    pub fn body<'a>(&'a self, t: theme::Tokens, in_library: &dyn Fn(&str) -> bool) -> Element<'a, Message> {
        let jobs: HashMap<String, Job> = self.queue.snapshot().into_iter().map(|job| (job.id.clone(), job)).collect();

        if self.items.is_empty() {
            let message = if self.loading {
                "Loading wallpapers...".to_string()
            } else if let Some(err) = &self.error {
                format!("Couldn't load wallpapers: {err}")
            } else {
                "No wallpapers found.".to_string()
            };
            let mut content = column![text(message).color(t.muted_foreground)].spacing(12);
            if self.error.is_some() && !self.loading {
                content = content.push(button(text("Try again")).style(button::primary).on_press(Message::Reload));
            }
            return container(content).padding(24).into();
        }

        let cards: Vec<Element<'a, Message>> = self
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let job_id = format!("{}:{}", item.source, item.id);
                card(index, item, self.thumbs.get(&item.id), jobs.get(&job_id), in_library(&job_id), t)
            })
            .collect();

        let fluid = grid::Grid::with_children(cards)
            .fluid(super::gallery::FLUID_COLUMN_WIDTH)
            .spacing(16)
            .height(grid::aspect_ratio(super::gallery::card_ratio(), 1.0));

        let mut page = column![container(fluid).padding([4, 4])].spacing(16);

        if self.loading {
            page = page.push(text("Loading more...").color(t.muted_foreground));
        } else if let Some(err) = &self.error {
            page = page.push(
                row![
                    text(format!("Couldn't load more: {err}")).color(t.muted_foreground),
                    button(text("Try again")).style(button::secondary).on_press(Message::LoadMore),
                ]
                .spacing(12)
                .align_y(iced::Alignment::Center),
            );
        } else if self.has_more {
            page = page.push(button(text("Load more")).style(button::secondary).on_press(Message::LoadMore));
        }

        scrollable(container(page).padding([0, 4])).into()
    }
}

fn card<'a>(
    index: usize,
    item: &'a OnlineItem,
    thumb: Option<&'a PathBuf>,
    job: Option<&Job>,
    in_library: bool,
    t: theme::Tokens,
) -> Element<'a, Message> {
    let preview: Element<'a, Message> = match thumb {
        Some(path) => image(image::Handle::from_path(path)).content_fit(iced::ContentFit::Cover).into(),
        None => container(svg(icons::film()).width(32).height(32).style(move |_theme, _status| svg::Style { color: Some(t.muted_foreground) }))
            .center(Length::Fill)
            .style(move |_theme| container::Style { background: Some(Background::Color(t.muted)), ..container::Style::default() })
            .into(),
    };

    let badge: Element<'a, Message> = if item.quality.is_empty() {
        Space::new().into()
    } else {
        container(container(text(item.quality.clone()).size(11).color(iced::Color::WHITE)).padding([2, 6]).style(|_theme| container::Style {
            background: Some(Background::Color(iced::Color { a: 0.6, ..iced::Color::BLACK })),
            border: Border { radius: 6.0.into(), ..Border::default() },
            ..container::Style::default()
        }))
        .padding(6)
        .into()
    };

    let image_area = container(stack![preview, badge]).clip(true).width(Length::Fill).height(Length::Fill);

    let action: Element<'a, Message> = if in_library {
        text("In your library").size(12).color(t.success).into()
    } else {
        match job {
            None => button(text("Download").size(12))
                .style(button::primary)
                .width(Length::Fill)
                .on_press(Message::Download(index))
                .into(),
            Some(job) => job_status(job, t),
        }
    };

    // Fixed height so a text-only status ("In your library") lines up with
    // neighbouring cards that show a button.
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

/// One-line status for a queued job plus the buttons that apply to it.
fn job_status<'a>(job: &Job, t: theme::Tokens) -> Element<'a, Message> {
    let small = |label: &'static str, message: Message| button(text(label).size(12)).style(button::text).padding([2, 6]).on_press(message);

    let (label, buttons): (String, Vec<Element<'a, Message>>) = match job.status {
        Status::Queued => ("Queued".into(), vec![small("Cancel", Message::Cancel(job.id.clone())).into()]),
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
