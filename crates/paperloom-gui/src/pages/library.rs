use std::collections::HashMap;
use std::time::Duration;

use iced::widget::{button, column, container, grid, image, mouse_area, row, svg, text, Space};
use iced::widget::image::Allocation;
use iced::{Background, Border, Element, Length, Shadow, Subscription, Task};

use paperloom_common::{Library, Settings};

use crate::{icons, theme};

pub struct State {
    library: Library,
    search: String,
    status: Option<String>,
    /// Set when "Remove" is clicked from a tile's kebab menu, before the
    /// user confirms — the tile switches to a "really remove? / cancel"
    /// prompt instead of deleting immediately. Direct response to a
    /// flagged UX anti-pattern ("no confirmation before delete") — see
    /// DESIGN.md.
    confirm_remove: Option<usize>,
    /// The tile whose kebab (top-right "⋮") menu is currently open, if
    /// any — only one open at a time, so opening a different tile's menu
    /// closes whatever was open.
    menu_open: Option<usize>,
    /// The tile under the cursor — the only one that animates.
    hovered: Option<usize>,
    /// Frames elapsed since the hover began (see `ping_pong`).
    preview_tick: usize,
    /// GPU-resident preview frames for recently hovered tiles, keyed by
    /// library index. iced uploads images to the GPU asynchronously, so a
    /// frame that has never been drawn shows nothing for a frame or two —
    /// exactly the black flicker when animating. Holding an `Allocation`
    /// per frame guarantees it draws immediately; playback only starts
    /// once every frame of the tile is allocated.
    preview_cache: HashMap<usize, Vec<Option<Allocation>>>,
    /// The Stop button was clicked and the choice is showing.
    confirm_stop: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    SearchChanged(String),
    ImportClicked,
    MenuToggled(usize),
    RemoveClicked(usize),
    ConfirmRemoveClicked(usize),
    CancelRemoveClicked,
    SetAsWallpaperClicked(usize),
    /// Opens the "stop wallpaper / stop and quit" choice.
    StopClicked,
    StopWallpaper,
    StopAndQuit,
    CancelStop,
    PreviewTick,
    TileHovered(usize),
    TileUnhovered(usize),
    FrameAllocated { entry: usize, frame: usize, result: Result<Allocation, iced::widget::image::Error> },
    OpenPreview(usize),
}

impl State {
    pub fn new() -> Self {
        Self {
            library: Library::load(),
            search: String::new(),
            status: None,
            confirm_remove: None,
            menu_open: None,
            confirm_stop: false,
            hovered: None,
            preview_tick: 0,
            preview_cache: HashMap::new(),
        }
    }

    /// Ticks only while the library tab is on screen *and* a tile with
    /// motion frames is hovered — at rest (the normal case) this page
    /// schedules nothing, so an open-but-idle library costs nothing.
    pub fn subscription(&self, active: bool) -> Subscription<Message> {
        let animating = self.hovered.is_some_and(|index| self.preview_ready(index));

        if active && animating {
            iced::time::every(Duration::from_millis(100)).map(|_| Message::PreviewTick)
        } else {
            Subscription::none()
        }
    }

    fn preview_ready(&self, index: usize) -> bool {
        self.preview_cache.get(&index).is_some_and(|frames| frames.len() > 1 && frames.iter().all(Option::is_some))
    }

    pub fn update(&mut self, settings: &Settings, message: Message) -> Task<Message> {
        match message {
            Message::SearchChanged(value) => self.search = value,
            Message::ImportClicked => {
                let picked = rfd::FileDialog::new()
                    .set_title("Import wallpapers")
                    .add_filter("MP4 video", &["mp4"])
                    .pick_files();

                if let Some(paths) = picked {
                    let total = paths.len();
                    let mut failed = Vec::new();
                    for path in &paths {
                        if let Err(err) = self.library.import(path, settings.import_mode) {
                            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                            failed.push(format!("{name} ({err})"));
                        }
                    }
                    self.preview_cache.clear();
                    self.status = if failed.is_empty() {
                        None
                    } else if failed.len() == total {
                        Some(format!("Couldn't import: {}", failed.join(", ")))
                    } else {
                        Some(format!("Imported {}/{total}. Failed: {}", total - failed.len(), failed.join(", ")))
                    };
                }
            }
            Message::MenuToggled(index) => {
                self.menu_open = if self.menu_open == Some(index) { None } else { Some(index) };
            }
            Message::RemoveClicked(index) => {
                self.menu_open = None;
                self.confirm_remove = Some(index);
            }
            Message::CancelRemoveClicked => self.confirm_remove = None,
            Message::ConfirmRemoveClicked(index) => {
                self.confirm_remove = None;
                self.hovered = None;
                self.preview_cache.clear();
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
            Message::StopClicked => self.confirm_stop = true,
            Message::CancelStop => self.confirm_stop = false,
            // The engine exits on Stop, which brings the normal desktop
            // background back (see paperloom-engine's player).
            Message::StopWallpaper => {
                self.confirm_stop = false;
                self.status = match crate::engine_client::stop() {
                    Ok(()) => Some("Wallpaper stopped — your normal desktop background is back.".to_string()),
                    Err(err) => Some(format!("Couldn't stop the wallpaper: {err}")),
                };
            }
            Message::StopAndQuit => {
                let _ = crate::engine_client::stop();
                return iced::exit();
            }
            Message::PreviewTick => self.preview_tick = self.preview_tick.wrapping_add(1),
            Message::TileHovered(index) => {
                // Restart from the poster frame so playback begins exactly
                // where the static image already was — no visible jump.
                self.hovered = Some(index);
                self.preview_tick = 0;

                let Some(entry) = self.library.entries.get(index) else { return Task::none() };
                if entry.preview_frames.len() > 1 && !self.preview_cache.contains_key(&index) {
                    // Keep GPU memory bounded: only the tile being hovered
                    // (plus nothing else) holds its frames.
                    self.preview_cache.clear();
                    self.preview_cache.insert(index, vec![None; entry.preview_frames.len()]);
                    return Task::batch(entry.preview_frames.iter().enumerate().map(|(frame, path)| {
                        iced::widget::image::allocate(image::Handle::from_path(path))
                            .map(move |result| Message::FrameAllocated { entry: index, frame, result })
                    }));
                }
            }
            Message::FrameAllocated { entry, frame, result } => {
                if let (Ok(allocation), Some(frames)) = (result, self.preview_cache.get_mut(&entry)) {
                    if let Some(slot) = frames.get_mut(frame) {
                        *slot = Some(allocation);
                    }
                }
            }
            Message::OpenPreview(index) => {
                if let Some(entry) = self.library.entries.get(index) {
                    if let Err(err) = crate::engine_client::preview(&entry.path) {
                        self.status = Some(format!("Couldn't open the preview: {err}"));
                    }
                }
            }
            Message::TileUnhovered(index) => {
                if self.hovered == Some(index) {
                    self.hovered = None;
                }
            }
        }
        Task::none()
    }

    /// Imports a finished online download, titled and tagged with its
    /// origin so the online browser can mark it "In library".
    pub fn import_downloaded(&mut self, settings: &Settings, job: &paperloom_fetch::Job) -> Result<(), String> {
        self.preview_cache.clear();
        self.hovered = None;
        self.library
            .import_named(&job.final_path(), settings.import_mode, &job.title, &job.id)
            .map_err(|err| format!("Couldn't add \"{}\" to your library: {err}", job.title))
    }

    pub fn set_status(&mut self, message: String) {
        self.status = Some(message);
    }

    pub fn has_source(&self, source_id: &str) -> bool {
        self.library.contains_source(source_id)
    }

    /// Shows only wallpapers whose name contains `filter` (set from the
    /// global search; "" shows everything).
    pub fn set_filter(&mut self, filter: String) {
        self.search = filter;
    }

    /// `(index, name)` of every wallpaper, for the global search.
    pub fn names(&self) -> impl Iterator<Item = (usize, &str)> {
        self.library.entries.iter().enumerate().map(|(index, entry)| (index, entry.display_name.as_str()))
    }

    pub fn header<'a>(&'a self) -> Element<'a, Message> {
        row![button(crate::ui::label("Import wallpapers...")).style(button::primary).on_press(Message::ImportClicked), self.stop_controls()]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .wrap()
            .vertical_spacing(8)
            .into()
    }

    /// "Stop wallpaper", which turns into the choice between stopping only
    /// the wallpaper (Paperloom stays open) and stopping it and quitting.
    fn stop_controls<'a>(&self) -> Element<'a, Message> {
        if !self.confirm_stop {
            return button(crate::ui::label("Stop wallpaper")).style(button::secondary).on_press(Message::StopClicked).into();
        }
        row![
            button(crate::ui::label("Stop wallpaper only")).style(button::secondary).on_press(Message::StopWallpaper),
            button(crate::ui::label("Stop and quit Paperloom")).style(button::danger).on_press(Message::StopAndQuit),
            button(crate::ui::label("Cancel")).style(button::text).on_press(Message::CancelStop),
        ]
        .spacing(6)
        .wrap()
        .vertical_spacing(6)
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
            let fluid = grid::Grid::with_children(matches.into_iter().map(|(index, entry)| {
                let confirming = self.confirm_remove == Some(index);
                let menu_open = self.menu_open == Some(index);
                let tick = if self.hovered == Some(index) && self.preview_ready(index) { self.preview_tick } else { 0 };
                tile(index, entry, confirming, menu_open, tick, t)
            }))
            .fluid(super::gallery::FLUID_COLUMN_WIDTH)
            .spacing(16)
            .height(grid::aspect_ratio(super::gallery::card_ratio(), 1.0));

            crate::ui::scroll_page(fluid).into()
        };

        let mut content = column![].spacing(16);
        if !self.search.is_empty() {
            content = content.push(
                row![
                    text(format!("Wallpapers matching “{}”", self.search)).color(t.foreground),
                    button(crate::ui::label("Show all").size(12)).style(button::text).padding([2, 6]).on_press(Message::SearchChanged(String::new())),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .wrap(),
            );
        }
        content = content.push(body);

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

/// A single gallery card: a preview image sized to the real desktop's
/// aspect ratio, a kebab menu in its top-right corner for secondary
/// actions (currently just "Remove"), and a fixed bottom bar with the
/// primary "Set as wallpaper" action — buttons live in the card's chrome
/// rather than appearing only on hover, so they work the same on a
/// touchpad as a mouse and are visible at a glance.
fn tile<'a>(
    index: usize,
    entry: &'a paperloom_common::LibraryEntry,
    confirming: bool,
    menu_open: bool,
    preview_tick: usize,
    t: theme::Tokens,
) -> Element<'a, Message> {
    // Tick 0 is the poster (also `preview_frames[0]`), so a tile at rest
    // and a tile that just started playing show the identical image.
    // Entries without frames fall back to the plain `thumbnail`, then to
    // an icon if extraction failed outright.
    let current_frame = if entry.preview_frames.is_empty() {
        entry.thumbnail.as_ref()
    } else {
        entry.preview_frames.get(ping_pong(preview_tick, entry.preview_frames.len()))
    };

    let thumb: Element<'a, Message> = match current_frame {
        Some(path) => image(image::Handle::from_path(path)).content_fit(iced::ContentFit::Cover).into(),
        None => container(
            svg(icons::film()).width(32).height(32).style(move |_theme, _status| svg::Style { color: Some(t.muted_foreground) }),
        )
        .center(Length::Fill)
        .style(move |_theme| container::Style { background: Some(Background::Color(t.muted)), ..container::Style::default() })
        .into(),
    };

    let kebab = button(svg(icons::ellipsis()).width(16).height(16).style(|_theme, _status| svg::Style { color: Some(iced::Color::WHITE) }))
        .style(move |_theme, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(Background::Color(iced::Color { a: if hovered || menu_open { 0.65 } else { 0.4 }, ..iced::Color::BLACK })),
                text_color: iced::Color::WHITE,
                border: Border { radius: 6.0.into(), ..Border::default() },
                shadow: Shadow::default(),
                snap: false,
            }
        })
        .padding(4)
        .on_press(Message::MenuToggled(index));

    // The menu hangs below the button. (Not a `stack`: a stack is only as
    // big as its first layer, so the menu would be squeezed to nothing.)
    let kebab_corner: Element<'a, Message> = if menu_open {
        column![
            kebab,
            container(
                button(crate::ui::label("Remove").size(13).color(t.destructive))
                    .style(button::text)
                    .padding([6, 10])
                    .on_press(Message::RemoveClicked(index)),
            )
            .style(move |_theme| container::Style {
                background: Some(Background::Color(t.surface)),
                border: Border { color: t.border, width: 1.0, radius: 8.0.into() },
                shadow: Shadow { color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.3), offset: iced::Vector::new(0.0, 2.0), blur_radius: 8.0 },
                ..container::Style::default()
            }),
        ]
        .spacing(4)
        .align_x(iced::Alignment::End)
        .padding(6)
        .into()
    } else {
        container(kebab).padding(6).into()
    };

    let confirm_overlay: Element<'a, Message> = if confirming {
        container(
            column![
                text("Remove this wallpaper?").color(iced::Color::WHITE).size(13),
                row![
                    button(crate::ui::label("Remove").size(13)).style(button::danger).on_press(Message::ConfirmRemoveClicked(index)),
                    button(crate::ui::label("Cancel").size(13)).style(button::text).on_press(Message::CancelRemoveClicked),
                ]
                .spacing(8),
            ]
            .spacing(8)
            .align_x(iced::Alignment::Center),
        )
        .center(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(iced::Color { a: 0.82, ..iced::Color::BLACK })),
            border: Border { radius: 0.0.into(), ..Border::default() },
            ..container::Style::default()
        })
        .into()
    } else {
        container(Space::new().width(Length::Fill).height(Length::Fill)).into()
    };

    let image_area = container(iced::widget::stack![
        thumb,
        container(kebab_corner).align_x(iced::alignment::Horizontal::Right).width(Length::Fill),
        confirm_overlay,
    ])
    .clip(true)
    .style(move |_theme| container::Style {
        border: Border { color: t.border, width: 1.0, radius: Border::default().radius },
        ..container::Style::default()
    })
    .width(Length::Fill)
    .height(Length::Fill);

    let chrome = column![
        text(entry.display_name.clone()).size(13).color(t.foreground),
        row![
            button(crate::ui::label("Set as wallpaper").size(12)).style(button::primary).width(Length::Fill).on_press(Message::SetAsWallpaperClicked(index)),
        ]
        .spacing(8),
    ]
    .spacing(8)
    .padding([8, 2]);

    mouse_area(
        container(
            column![image_area, chrome]
                .spacing(0)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .style(move |_theme| container::Style {
            background: Some(Background::Color(t.surface)),
            border: Border { color: t.border, width: 1.0, radius: 12.0.into() },
            shadow: Shadow { color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.15), offset: iced::Vector::new(0.0, 2.0), blur_radius: 8.0 },
            ..container::Style::default()
        })
        .padding(0),
    )
    .on_enter(Message::TileHovered(index))
    .on_exit(Message::TileUnhovered(index))
    .on_double_click(Message::OpenPreview(index))
    .into()
}

/// Maps an ever-increasing tick to a frame index that plays forward then
/// backward (0,1,..,n-1,n-2,..,1,0,1,..) — a short clip excerpt isn't
/// loopable, so replaying it backwards avoids a visible jump from the last
/// frame to the first.
fn ping_pong(tick: usize, frames: usize) -> usize {
    if frames <= 1 {
        return 0;
    }
    let period = 2 * (frames - 1);
    let position = tick % period;
    if position < frames {
        position
    } else {
        period - position
    }
}

#[cfg(test)]
mod tests {
    use super::ping_pong;

    #[test]
    fn ping_pong_bounces_without_repeating_the_ends() {
        let sequence: Vec<usize> = (0..10).map(|tick| ping_pong(tick, 4)).collect();
        assert_eq!(sequence, [0, 1, 2, 3, 2, 1, 0, 1, 2, 3]);
        assert_eq!(ping_pong(5, 1), 0);
        assert_eq!(ping_pong(5, 0), 0);
    }
}
