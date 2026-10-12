use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Background, Border, Element, Length, Task};

use paperloom_common::{Library, Settings};

use crate::theme;

pub struct State {
    library: Library,
    search: String,
    status: Option<String>,
    /// Set when "Remove" is clicked, before the user confirms — the row
    /// switches to a "really remove? / cancel" prompt instead of deleting
    /// immediately. Direct response to a flagged UX anti-pattern
    /// ("no confirmation before delete") — see DESIGN.md.
    confirm_remove: Option<usize>,
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
}

impl State {
    pub fn new() -> Self {
        Self {
            library: Library::load(),
            search: String::new(),
            status: None,
            confirm_remove: None,
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

        let mut list = column![].spacing(8);
        if matches.is_empty() {
            let message = if self.library.entries.is_empty() {
                "No wallpapers yet. Import an MP4 to get started."
            } else {
                "No wallpapers match your search."
            };
            list = list.push(text(message));
        } else {
            for (index, entry) in matches {
                let actions: Element<'a, Message> = if self.confirm_remove == Some(index) {
                    row![
                        text("Remove this wallpaper?"),
                        button(text("Remove")).style(button::danger).on_press(Message::ConfirmRemoveClicked(index)),
                        button(text("Cancel")).style(button::text).on_press(Message::CancelRemoveClicked),
                    ]
                    .spacing(8)
                    .into()
                } else {
                    row![
                        button(text("Set as wallpaper")).style(button::primary).on_press(Message::SetAsWallpaperClicked(index)),
                        button(text("Remove")).style(button::text).on_press(Message::RemoveClicked(index)),
                    ]
                    .spacing(8)
                    .into()
                };

                let entry_row = row![text(entry.display_name.clone()).width(Length::Fill), actions].spacing(8).padding(4);

                list = list.push(card(entry_row.into(), t));
            }
        }

        let mut content = column![scrollable(list)].spacing(16);

        if let Some(status) = &self.status {
            content = content.push(text(status));
        }

        content.into()
    }
}

/// A subtle elevated row — `theme::Tokens::surface` background with a
/// thin border, rounded corners. Used for every library entry so the list
/// reads as distinct cards instead of plain text rows with no boundary.
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
