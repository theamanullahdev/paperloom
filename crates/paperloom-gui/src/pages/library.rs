use std::time::{Duration, Instant};

use iced::widget::{button, column, container, grid, image, mouse_area, row, scrollable, svg, text, text_input, Space};
use iced::{Animation, Background, Border, Element, Length, Shadow, Subscription, Task};

use paperloom_common::{Library, Settings};

use crate::{icons, theme};

pub struct State {
    library: Library,
    search: String,
    status: Option<String>,
    /// Set when "Remove" is clicked, before the user confirms — the tile
    /// switches to a "really remove? / cancel" prompt instead of deleting
    /// immediately. Direct response to a flagged UX anti-pattern
    /// ("no confirmation before delete") — see DESIGN.md.
    confirm_remove: Option<usize>,
    /// The tile the cursor is currently over, if any — drives the hover
    /// overlay fade below. Only one can be hovered at a time, so a single
    /// `Option` (rather than per-tile state) is enough.
    hovered: Option<usize>,
    hover_fade: Animation<bool>,
}

#[derive(Debug, Clone)]
pub enum Message {
    SearchChanged(String),
    ImportClicked,
    RemoveClicked(usize),
    ConfirmRemoveClicked(usize),
    CancelRemoveClicked,
    SetAsWallpaperClicked(usize),
    StopClicked,
    TileHovered(usize),
    TileUnhovered(usize),
    Tick,
}

impl State {
    pub fn new() -> Self {
        Self {
            library: Library::load(),
            search: String::new(),
            status: None,
            confirm_remove: None,
            hovered: None,
            hover_fade: Animation::new(false).quick(),
        }
    }

    /// Only subscribes to frame ticks while a tile's hover overlay is
    /// actively fading, so an idle library view costs nothing extra.
    pub fn subscription(&self) -> Subscription<Message> {
        if self.hover_fade.is_animating(Instant::now()) {
            iced::time::every(Duration::from_millis(16)).map(|_| Message::Tick)
        } else {
            Subscription::none()
        }
    }

    pub fn update(&mut self, settings: &Settings, message: Message) -> Task<Message> {
        match message {
            Message::SearchChanged(value) => self.search = value,
            Message::ImportClicked => {
                let picked = rfd::FileDialog::new()
                    .set_title("Import wallpaper")
                    .add_filter("MP4 video", &["mp4"])
                    .pick_file();

                if let Some(path) = picked {
                    self.status = match self.library.import(&path, settings.import_mode) {
                        Ok(()) => None,
                        Err(err) => Some(format!("Couldn't import that file: {err}")),
                    };
                }
            }
            Message::RemoveClicked(index) => self.confirm_remove = Some(index),
            Message::CancelRemoveClicked => self.confirm_remove = None,
            Message::ConfirmRemoveClicked(index) => {
                self.confirm_remove = None;
                self.status = match self.library.remove(index) {
                    Ok(()) => None,
                    Err(err) => Some(format!("Couldn't remove that wallpaper: {err}")),
                };
            }
            Message::SetAsWallpaperClicked(index) => {
                if let Some(entry) = self.library.entries.get(index) {
                    self.status = match crate::engine_client::play(&entry.path) {
                        Ok(()) => Some(format!("Now playing: {}", entry.display_name)),
                        Err(err) => Some(format!("Couldn't start playback: {err}")),
                    };
                }
            }
            Message::StopClicked => {
                self.status = match crate::engine_client::stop() {
                    Ok(()) => Some("Wallpaper stopped.".to_string()),
                    Err(err) => Some(format!("Couldn't stop playback: {err}")),
                };
            }
            Message::TileHovered(index) => {
                self.hovered = Some(index);
                self.hover_fade = Animation::new(false).quick();
                self.hover_fade.go_mut(true, Instant::now());
            }
            Message::TileUnhovered(index) => {
                if self.hovered == Some(index) {
                    self.hover_fade.go_mut(false, Instant::now());
                }
            }
            Message::Tick => {}
        }
        Task::none()
    }

    pub fn header<'a>(&'a self) -> Element<'a, Message> {
        row![
            text_input("Search your library...", &self.search)
                .on_input(Message::SearchChanged)
                .padding(8)
                .width(Length::Fixed(240.0)),
            button(text("Import wallpaper...")).style(button::primary).on_press(Message::ImportClicked),
            button(text("Stop wallpaper")).style(button::secondary).on_press(Message::StopClicked),
        ]
        .spacing(8)
        .into()
    }

    pub fn body<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let query = self.search.to_lowercase();
        let matches: Vec<_> = self
            .library
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| query.is_empty() || entry.display_name.to_lowercase().contains(&query))
            .collect();

        let body: Element<'a, Message> = if matches.is_empty() {
            let message = if self.library.entries.is_empty() {
                "No wallpapers yet. Import an MP4 to get started."
            } else {
                "No wallpapers match your search."
            };
            container(text(message).color(t.muted_foreground)).padding(24).into()
        } else {
            let now = Instant::now();
            let fluid = grid::Grid::with_children(matches.into_iter().map(|(index, entry)| {
                let fade = if self.hovered == Some(index) { self.hover_fade.interpolate(0.0_f32, 1.0_f32, now) } else { 0.0 };
                let confirming = self.confirm_remove == Some(index);
                tile(index, entry, fade, confirming, t)
            }))
            .fluid(260)
            .spacing(16)
            .height(grid::aspect_ratio(16, 10));

            scrollable(container(fluid).padding([4, 4])).into()
        };

        let mut content = column![body].spacing(16);

        if let Some(status) = &self.status {
            content = content.push(
                container(text(status).color(t.foreground))
                    .padding([8, 12])
                    .style(move |_theme| container::Style {
                        background: Some(Background::Color(t.muted)),
                        border: Border { radius: 8.0.into(), ..Border::default() },
                        ..container::Style::default()
                    }),
            );
        }

        content.into()
    }
}

/// A single gallery tile: thumbnail (or a film-icon placeholder), tinted on
/// hover with the title and action buttons fading in over it — a video
/// wallpaper's defining trait is that it moves, so the grid should read as
/// a gallery of previews, not a file list with buttons bolted on.
fn tile<'a>(
    index: usize,
    entry: &'a paperloom_common::LibraryEntry,
    fade: f32,
    confirming: bool,
    t: theme::Tokens,
) -> Element<'a, Message> {
    let thumb: Element<'a, Message> = match &entry.thumbnail {
        Some(path) => image(image::Handle::from_path(path)).content_fit(iced::ContentFit::Cover).into(),
        None => container(
            svg(icons::film()).width(32).height(32).style(move |_theme, _status| svg::Style { color: Some(t.muted_foreground) }),
        )
        .center(Length::Fill)
        .style(move |_theme| container::Style { background: Some(Background::Color(t.muted)), ..container::Style::default() })
        .into(),
    };

    // Below a visibility floor, don't build the overlay's buttons/icons at
    // all — a container's background alpha fading doesn't carry its
    // children with it, so a "fade-in" built only from background alpha
    // left every tile's badge and action buttons fully opaque *all the
    // time*, hover or not. Gating actual presence on `fade`, not just its
    // background color, is what makes "hidden until hovered" real.
    let overlay: Element<'a, Message> = if confirming {
        container(
            column![
                text("Remove this wallpaper?").color(iced::Color::WHITE).size(13),
                row![
                    button(text("Remove").size(13)).style(button::danger).on_press(Message::ConfirmRemoveClicked(index)),
                    button(text("Cancel").size(13)).style(button::text).on_press(Message::CancelRemoveClicked),
                ]
                .spacing(8),
            ]
            .spacing(8)
            .align_x(iced::Alignment::Center),
        )
        .center(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(iced::Color { a: 0.82, ..iced::Color::BLACK })),
            border: Border { radius: 12.0.into(), ..Border::default() },
            ..container::Style::default()
        })
        .into()
    } else if fade > 0.01 {
        let play_badge = svg(icons::play()).width(36).height(36).style(|_theme, _status| svg::Style { color: Some(iced::Color::WHITE) });

        let actions = row![
            button(text("Set as wallpaper").size(12)).style(button::primary).on_press(Message::SetAsWallpaperClicked(index)),
            button(svg(icons::trash()).width(16).height(16).style(|_theme, _status| svg::Style { color: Some(iced::Color::WHITE) }))
                .style(button::text)
                .on_press(Message::RemoveClicked(index)),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);

        container(
            column![container(play_badge).center(Length::Fill), container(actions).padding([0, 4])]
                .spacing(8)
                .padding(10)
                .height(Length::Fill),
        )
        .style(move |_theme| container::Style {
            background: Some(Background::Color(iced::Color { a: fade * 0.72, ..iced::Color::BLACK })),
            border: Border { radius: 12.0.into(), ..Border::default() },
            ..container::Style::default()
        })
        .into()
    } else {
        container(Space::new().width(Length::Fill).height(Length::Fill)).into()
    };

    let name_strip = container(text(entry.display_name.clone()).size(12).color(iced::Color::WHITE))
        .padding([4, 8])
        .width(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(Background::Color(iced::Color { a: 0.55, ..iced::Color::BLACK })),
            ..container::Style::default()
        });

    let stack = iced::widget::stack![thumb, overlay, container(name_strip).align_y(iced::alignment::Vertical::Bottom).height(Length::Fill)];

    mouse_area(
        container(stack)
            .clip(true)
            .style(move |_theme| container::Style {
                border: Border { color: t.border, width: 1.0, radius: 12.0.into() },
                shadow: if fade > 0.0 {
                    Shadow { color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.35 * fade), offset: iced::Vector::new(0.0, 4.0), blur_radius: 16.0 }
                } else {
                    Shadow::default()
                },
                ..container::Style::default()
            }),
    )
    .on_enter(Message::TileHovered(index))
    .on_exit(Message::TileUnhovered(index))
    .into()
}
