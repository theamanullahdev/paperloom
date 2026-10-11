use iced::widget::{button, column, container, row, text};
use iced::{Element, Length, Subscription, Task, Theme};

use paperloom_common::Settings;

use crate::pages::{library, settings};

pub struct App {
    page: Page,
    settings: Settings,
    settings_page: settings::State,
    library_page: library::State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Page {
    Library,
    Settings,
}

#[derive(Debug, Clone)]
pub enum Message {
    SwitchPage(Page),
    Settings(settings::Message),
    Library(library::Message),
}

impl App {
    pub fn new() -> Self {
        Self {
            page: Page::Library,
            settings: paperloom_common::load(),
            settings_page: settings::State::new(),
            library_page: library::State::new(),
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SwitchPage(page) => {
                self.page = page;
                Task::none()
            }
            Message::Settings(msg) => self.settings_page.update(&mut self.settings, msg).map(Message::Settings),
            Message::Library(msg) => self.library_page.update(&self.settings, msg).map(Message::Library),
        }
    }

    pub fn theme(&self) -> Theme {
        crate::theme::from_mode(self.settings.theme)
    }

    /// Only the Settings page needs frame ticks right now (to animate the
    /// search-result highlight) — delegate rather than run a global timer
    /// all the time, so idling on the Library page costs nothing extra.
    pub fn subscription(&self) -> Subscription<Message> {
        self.settings_page.subscription().map(Message::Settings)
    }

    pub fn view(&self) -> Element<'_, Message> {
        let nav_button = |label: &'static str, page: Page| {
            button(text(label)).on_press(Message::SwitchPage(page)).width(Length::Fill)
        };

        let nav = container(
            column![nav_button("Library", Page::Library), nav_button("Settings", Page::Settings)]
                .spacing(8)
                .padding(12),
        )
        .width(Length::Fixed(180.0));

        let content = match self.page {
            Page::Library => self.library_page.view().map(Message::Library),
            Page::Settings => self.settings_page.view(&self.settings).map(Message::Settings),
        };

        row![nav, content].into()
    }
}
