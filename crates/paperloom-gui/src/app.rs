use std::time::Duration;

use iced::widget::{button, column, container, row, svg, text, Space};
use iced::{Alignment, Background, Border, Color, Element, Length, Shadow, Subscription, Task, Theme};

use paperloom_common::Settings;

use crate::pages::{library, online, settings};
use crate::{icons, theme};

pub struct App {
    page: Page,
    settings: Settings,
    settings_page: settings::State,
    library_page: library::State,
    online_page: online::State,
    /// What the desktop says its light/dark preference is; only consulted
    /// when the user's theme setting is "System".
    system_theme: iced::theme::Mode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Page {
    Library,
    Online,
    Settings,
}

#[derive(Debug, Clone)]
pub enum Message {
    SwitchPage(Page),
    Settings(settings::Message),
    Library(library::Message),
    Online(online::Message),
    /// Fires while downloads are in flight: moves finished files into the
    /// library.
    DownloadsTick,
    SystemTheme(iced::theme::Mode),
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let app = Self {
            page: Page::Library,
            settings: paperloom_common::load(),
            settings_page: settings::State::new(),
            library_page: library::State::new(),
            online_page: online::State::new(),
            system_theme: iced::theme::Mode::None,
        };
        (app, iced::system::theme().map(Message::SystemTheme))
    }

    fn effective_theme(&self) -> paperloom_common::ThemeMode {
        theme::resolve(self.settings.theme, self.system_theme)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SwitchPage(page) => {
                self.page = page;
                if page == Page::Online {
                    self.online_page.ensure_loaded().map(Message::Online)
                } else {
                    Task::none()
                }
            }
            Message::Settings(msg) => self.settings_page.update(&mut self.settings, msg).map(Message::Settings),
            Message::Library(msg) => self.library_page.update(&self.settings, msg).map(Message::Library),
            Message::Online(msg) => self.online_page.update(msg).map(Message::Online),
            Message::SystemTheme(mode) => {
                self.system_theme = mode;
                Task::none()
            }
            Message::DownloadsTick => {
                self.import_finished_downloads();
                Task::none()
            }
        }
    }

    /// Adds completed downloads to the library. The downloaded copy is
    /// deleted afterwards unless the user's import mode is "reference in
    /// place", in which case the library points at it and it must stay.
    fn import_finished_downloads(&mut self) {
        let keep_file = self.settings.import_mode == paperloom_common::ImportMode::ReferenceInPlace;
        for job in self.online_page.queue().finished() {
            match self.library_page.import_downloaded(&self.settings, &job) {
                Ok(()) => self.online_page.queue().acknowledge(&job.id, !keep_file),
                Err(message) => {
                    self.library_page.set_status(message);
                    // Leave the file in place, but stop retrying the import
                    // on every tick.
                    self.online_page.queue().acknowledge(&job.id, false);
                }
            }
        }
    }

    pub fn theme(&self) -> Theme {
        theme::from_mode(self.effective_theme())
    }

    /// Both pages only need frame ticks while something on them is
    /// actively animating (settings-highlight fade, gallery hover fade) —
    /// each delegates its own subscription so an idle page costs nothing.
    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::system::theme_changes().map(Message::SystemTheme),
            self.settings_page.subscription().map(Message::Settings),
            self.library_page.subscription(self.page == Page::Library).map(Message::Library),
            if self.online_page.queue().is_busy() || !self.online_page.queue().finished().is_empty() {
                iced::time::every(Duration::from_secs(1)).map(|_| Message::DownloadsTick)
            } else {
                Subscription::none()
            },
        ])
    }

    pub fn view(&self) -> Element<'_, Message> {
        let t = theme::tokens(self.effective_theme());

        let sidebar = container(
            column![
                text("Paperloom").font(theme::DISPLAY_FONT).size(20).color(t.primary),
                column![
                    nav_item(icons::library(), "Library", Page::Library, self.page == Page::Library, t),
                    nav_item(icons::globe(), "Online", Page::Online, self.page == Page::Online, t),
                    nav_item(icons::settings(), "Settings", Page::Settings, self.page == Page::Settings, t),
                ]
                .spacing(4),
            ]
            .spacing(20)
            .padding(16)
            .width(Length::Fixed(200.0)),
        )
        .style(move |_theme| container::Style {
            background: Some(Background::Color(t.surface)),
            border: Border { color: t.border, width: 1.0, radius: 0.0.into() },
            shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.25), offset: iced::Vector::new(2.0, 0.0), blur_radius: 12.0 },
            ..container::Style::default()
        })
        .height(Length::Fill);

        let (title, header, body) = match self.page {
            Page::Library => ("Library", self.library_page.header().map(Message::Library), self.library_page.body(t).map(Message::Library)),
            Page::Online => {
                let in_library = |id: &str| self.library_page.has_source(id);
                ("Online", self.online_page.header().map(Message::Online), self.online_page.body(t, &in_library).map(Message::Online))
            }
            Page::Settings => {
                ("Settings", self.settings_page.header().map(Message::Settings), self.settings_page.body(&self.settings, t).map(Message::Settings))
            }
        };

        let top_bar = container(
            row![
                text(title).font(theme::DISPLAY_FONT).size(22).color(t.foreground),
                Space::new().width(Length::Fill),
                header,
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .padding([16, 24])
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(t.surface)),
            border: Border { color: t.border, width: 0.0, radius: 0.0.into() },
            shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.2), offset: iced::Vector::new(0.0, 2.0), blur_radius: 10.0 },
            ..container::Style::default()
        });

        let content = column![top_bar, container(body).padding([8, 24]).width(Length::Fill).height(Length::Fill)]
            .width(Length::Fill)
            .height(Length::Fill);

        container(row![sidebar, content])
            .style(move |_theme| container::Style { background: Some(Background::Color(t.background)), ..container::Style::default() })
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

fn nav_item<'a>(icon: svg::Handle, label: &'static str, page: Page, active: bool, t: theme::Tokens) -> Element<'a, Message> {
    let icon_color = if active { t.primary } else { t.muted_foreground };
    let text_color = if active { t.primary } else { t.foreground };

    let content = row![
        svg(icon).width(18).height(18).style(move |_theme, _status| svg::Style { color: Some(icon_color) }),
        text(label).font(theme::BODY_FONT).size(14).color(text_color),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    button(content)
        .padding([10, 12])
        .width(Length::Fill)
        .on_press(Message::SwitchPage(page))
        .style(move |_theme, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            let background = if active {
                Some(Background::Color(Color { a: 0.12, ..t.primary }))
            } else if hovered {
                Some(Background::Color(t.muted))
            } else {
                None
            };
            button::Style {
                background,
                text_color,
                border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 8.0.into() },
                shadow: Shadow::default(),
                snap: false,
            }
        })
        .into()
}
