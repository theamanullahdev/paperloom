use iced::widget::{button, column, container, row, svg, text, Space};
use iced::{Alignment, Background, Border, Color, Element, Length, Shadow, Subscription, Task, Theme};

use paperloom_common::Settings;

use crate::pages::{library, settings};
use crate::{icons, theme};

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
        theme::from_mode(self.settings.theme)
    }

    /// Both pages only need frame ticks while something on them is
    /// actively animating (settings-highlight fade, gallery hover fade) —
    /// each delegates its own subscription so an idle page costs nothing.
    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            self.settings_page.subscription().map(Message::Settings),
            self.library_page.subscription().map(Message::Library),
        ])
    }

    pub fn view(&self) -> Element<'_, Message> {
        let t = theme::tokens(self.settings.theme);

        let sidebar = container(
            column![
                text("Paperloom").font(theme::DISPLAY_FONT).size(20).color(t.primary),
                column![
                    nav_item(icons::library(), "Library", Page::Library, self.page == Page::Library, t),
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
