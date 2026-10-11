use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Element, Length, Task};

use paperloom_common::{Library, Settings};

pub struct State {
    library: Library,
    search: String,
    status: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    SearchChanged(String),
    ImportClicked,
    RemoveClicked(usize),
    SetAsWallpaperClicked(usize),
    StopClicked,
}

impl State {
    pub fn new() -> Self {
        Self {
            library: Library::load(),
            search: String::new(),
            status: None,
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
            Message::RemoveClicked(index) => {
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

    pub fn view<'a>(&'a self) -> Element<'a, Message> {
        let search = text_input("Search your library...", &self.search)
            .on_input(Message::SearchChanged)
            .padding(8)
            .width(Length::Fill);

        let import_button = button("Import wallpaper...").on_press(Message::ImportClicked);
        let stop_button = button("Stop wallpaper").on_press(Message::StopClicked);

        let query = self.search.to_lowercase();
        let matches: Vec<_> = self
            .library
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| query.is_empty() || entry.display_name.to_lowercase().contains(&query))
            .collect();

        let mut list = column![].spacing(4);
        if matches.is_empty() {
            let message = if self.library.entries.is_empty() {
                "No wallpapers yet. Import an MP4 to get started."
            } else {
                "No wallpapers match your search."
            };
            list = list.push(text(message));
        } else {
            for (index, entry) in matches {
                let entry_row = row![
                    text(entry.display_name.clone()).width(Length::Fill),
                    button("Set as wallpaper").on_press(Message::SetAsWallpaperClicked(index)),
                    button("Remove").on_press(Message::RemoveClicked(index)),
                ]
                .spacing(8);
                list = list.push(container(entry_row).padding(8).width(Length::Fill));
            }
        }

        let mut content = column![row![search, import_button, stop_button].spacing(8), scrollable(list)]
            .spacing(16)
            .padding(16);

        if let Some(status) = &self.status {
            content = content.push(text(status));
        }

        content.into()
    }
}
