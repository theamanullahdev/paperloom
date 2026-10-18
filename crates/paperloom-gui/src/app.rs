use std::time::Duration;

use iced::widget::{button, column, container, mouse_area, opaque, responsive, row, scrollable, stack, svg, text, text_input, Space};
use iced::{Alignment, Background, Border, Color, Element, Length, Shadow, Size, Subscription, Task, Theme};

use paperloom_common::Settings;

use crate::pages::{downloads, library, online, settings, sources, updates};
use crate::ui::Width;
use crate::{icons, search, theme};

pub struct App {
    page: Page,
    settings: Settings,
    settings_page: settings::State,
    library_page: library::State,
    online_page: online::State,
    downloads_page: downloads::State,
    sources_page: sources::State,
    updates_page: updates::State,
    /// Sources changed since the Online tab last loaded; reload it when
    /// next opened so the change takes effect.
    online_stale: bool,
    /// What the desktop says its light/dark preference is; only consulted
    /// when the user's theme setting is "System".
    system_theme: iced::theme::Mode,
    /// The global search box (top bar) and whether its popup is showing.
    search: String,
    search_open: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Page {
    Library,
    Online,
    Downloads,
    Sources,
    Settings,
    Updates,
}

#[derive(Debug, Clone)]
pub enum Message {
    SwitchPage(Page),
    Settings(settings::Message),
    Library(library::Message),
    Online(online::Message),
    Downloads(downloads::Message),
    Sources(sources::Message),
    Updates(updates::Message),
    /// Fires while downloads are in flight: moves finished files into the
    /// library.
    DownloadsTick,
    SystemTheme(iced::theme::Mode),
    // --- Global search (see `search.rs`)
    SearchChanged(String),
    /// Enter in the search box: search online for what was typed.
    SearchSubmitted,
    SearchClosed,
    SearchOnline(String),
    GoToSetting(settings::SettingId),
    ShowInLibrary(String),
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let mut app = Self {
            page: Page::Library,
            settings: paperloom_common::load(),
            settings_page: settings::State::new(),
            library_page: library::State::new(),
            online_page: online::State::new(),
            downloads_page: downloads::State::default(),
            sources_page: sources::State::default(),
            updates_page: updates::State::new(),
            online_stale: false,
            system_theme: iced::theme::Mode::None,
            search: String::new(),
            search_open: false,
        };
        let check = if app.settings.updates.check_on_startup {
            app.updates_page.start_check(&app.settings).map(Message::Updates)
        } else {
            Task::none()
        };
        (app, Task::batch([iced::system::theme().map(Message::SystemTheme), check]))
    }

    fn effective_theme(&self) -> paperloom_common::ThemeMode {
        theme::resolve(self.settings.theme, self.system_theme)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SearchChanged(value) => {
                self.search_open = !value.trim().is_empty();
                self.search = value;
                Task::none()
            }
            Message::SearchSubmitted => {
                let query = self.search.trim().to_string();
                self.update(Message::SearchOnline(query))
            }
            Message::SearchClosed => {
                self.search_open = false;
                Task::none()
            }
            Message::SearchOnline(query) => {
                self.search_open = false;
                self.search = query.clone();
                self.page = Page::Online;
                self.online_stale = false;
                self.online_page.search(&self.settings, query).map(Message::Online)
            }
            Message::GoToSetting(id) => {
                self.search_open = false;
                self.page = if id == settings::SettingId::Updates { Page::Updates } else { Page::Settings };
                self.settings_page.update(&mut self.settings, settings::Message::ResultSelected(id)).map(Message::Settings)
            }
            Message::ShowInLibrary(name) => {
                self.search_open = false;
                self.page = Page::Library;
                self.library_page.set_filter(name);
                Task::none()
            }
            Message::SwitchPage(page) => {
                self.search_open = false;
                self.page = page;
                if page == Page::Online {
                    if std::mem::take(&mut self.online_stale) {
                        self.online_page.update(&self.settings, online::Message::Reload).map(Message::Online)
                    } else {
                        self.online_page.ensure_loaded(&self.settings).map(Message::Online)
                    }
                } else {
                    Task::none()
                }
            }
            Message::Settings(settings::Message::OpenUpdates) => {
                self.page = Page::Updates;
                Task::none()
            }
            Message::Settings(msg) => self.settings_page.update(&mut self.settings, msg).map(Message::Settings),
            Message::Updates(updates::Message::Back) => {
                self.page = Page::Settings;
                Task::none()
            }
            Message::Updates(msg) => self.updates_page.update(&mut self.settings, msg).map(Message::Updates),
            Message::Library(msg) => self.library_page.update(&self.settings, msg).map(Message::Library),
            Message::Online(msg) => self.online_page.update(&self.settings, msg).map(Message::Online),
            Message::Downloads(downloads::Message::Back) => {
                self.page = Page::Online;
                Task::none()
            }
            Message::Downloads(msg) => self.downloads_page.update(self.online_page.queue(), msg).map(Message::Downloads),
            Message::Sources(msg) => {
                self.online_stale = true;
                self.sources_page.update(&mut self.settings, msg).map(Message::Sources)
            }
            Message::SystemTheme(mode) => {
                self.system_theme = mode;
                Task::none()
            }
            Message::DownloadsTick => {
                self.import_finished_downloads();
                self.downloads_page.sample(&self.online_page.queue().snapshot());
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
            self.online_page.subscription(self.page == Page::Online).map(Message::Online),
            if self.online_page.queue().is_busy() || !self.online_page.queue().finished().is_empty() {
                // Smoother progress while the Downloads page is open.
                let period = if self.page == Page::Downloads { Duration::from_millis(300) } else { Duration::from_secs(1) };
                iced::time::every(period).map(|_| Message::DownloadsTick)
            } else {
                Subscription::none()
            },
            if self.updates_page.is_downloading() {
                iced::time::every(Duration::from_millis(300)).map(|_| Message::Updates(updates::Message::Tick))
            } else {
                Subscription::none()
            },
            if self.search_open {
                iced::event::listen_with(|event, _status, _window| match event {
                    iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                        ..
                    }) => Some(Message::SearchClosed),
                    _ => None,
                })
            } else {
                Subscription::none()
            },
            if self.downloads_page.is_dragging() {
                iced::event::listen_with(|event, _status, _window| match event {
                    iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                        Some(Message::Downloads(downloads::Message::DragEnd))
                    }
                    _ => None,
                })
            } else {
                Subscription::none()
            },
        ])
    }

    /// Always-visible shortcut to the Downloads page, with a count of
    /// downloads that aren't finished.
    fn downloads_button(&self, t: theme::Tokens) -> Element<'_, Message> {
        let pending = self.online_page.queue().snapshot().iter().filter(|job| job.status != paperloom_fetch::Status::Done).count();
        let active = self.page == Page::Downloads;
        let color = if active { t.primary } else { t.foreground };

        let mut content = row![svg(icons::download()).width(18).height(18).style(move |_theme, _status| svg::Style { color: Some(color) })]
            .spacing(6)
            .align_y(Alignment::Center);
        if pending > 0 {
            content = content.push(text(pending.to_string()).size(13).color(color));
        }

        button(content)
            .padding([8, 10])
            .on_press(Message::SwitchPage(Page::Downloads))
            .style(move |_theme, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: if active {
                        Some(Background::Color(Color { a: 0.12, ..t.primary }))
                    } else if hovered {
                        Some(Background::Color(t.muted))
                    } else {
                        None
                    },
                    text_color: color,
                    border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 8.0.into() },
                    shadow: Shadow::default(),
                    snap: false,
                }
            })
            .into()
    }

    /// Lays the app out for whatever window it's in: full sidebar, icon
    /// rail, or (narrow) navigation folded into the top bar; and when the
    /// window is very short, the whole content area scrolls instead of
    /// squashing the page to nothing.
    pub fn view(&self) -> Element<'_, Message> {
        responsive(move |size| self.layout(size)).into()
    }

    fn layout(&self, size: Size) -> Element<'_, Message> {
        let t = theme::tokens(self.effective_theme());
        let width = Width::of(size.width);
        let rail_width = match width {
            Width::Wide => 200.0,
            Width::Medium => 64.0,
            Width::Narrow => 0.0,
        };
        let content_width = (size.width - rail_width).max(0.0);

        let (title, header, body) = match self.page {
            Page::Library => ("Library", self.library_page.header().map(Message::Library), self.library_page.body(t).map(Message::Library)),
            Page::Online => {
                let in_library = |id: &str| self.library_page.has_source(id);
                ("Online", self.online_page.header(t).map(Message::Online), self.online_page.body(t, &in_library).map(Message::Online))
            }
            Page::Downloads => (
                "Downloads",
                self.downloads_page.header().map(Message::Downloads),
                self.downloads_page.body(t, self.online_page.queue().snapshot()).map(Message::Downloads),
            ),
            Page::Sources => ("Sources", Space::new().into(), self.sources_page.body(&self.settings, t).map(Message::Sources)),
            Page::Settings => ("Settings", Space::new().into(), self.settings_page.body(&self.settings, self.updates_page.summary(&self.settings), t).map(Message::Settings)),
            Page::Updates => ("Updates", self.updates_page.header().map(Message::Updates), self.updates_page.body(&self.settings, t).map(Message::Updates)),
        };

        let search_box = text_input("Search Paperloom & online wallpapers…", &self.search)
            .on_input(Message::SearchChanged)
            .on_submit(Message::SearchSubmitted)
            .padding(8)
            .width(if width == Width::Narrow { Length::Fill } else { Length::Fixed((content_width * 0.4).clamp(160.0, 380.0)) });

        let title_text = text(title).font(theme::DISPLAY_FONT).size(if width == Width::Narrow { 18 } else { 22 }).color(t.foreground).wrapping(text::Wrapping::None);

        // Everything in the bar wraps onto further lines rather than
        // squeezing: buttons keep their full labels at any width.
        let top_bar: Element<'_, Message> = if width == Width::Narrow {
            let mut nav = row![].spacing(4).align_y(Alignment::Center);
            for (icon, label, page) in nav_pages() {
                nav = nav.push(nav_item(icon, label, page, self.is_active(page), self.has_badge(page), false, t));
            }
            nav = nav.push(self.downloads_button(t));
            column![
                nav.wrap().vertical_spacing(4),
                search_box,
                row![title_text, header].spacing(12).align_y(Alignment::Center).wrap().vertical_spacing(8),
            ]
            .spacing(10)
            .into()
        } else {
            row![title_text, search_box, header, self.downloads_button(t)].spacing(12).align_y(Alignment::Center).wrap().vertical_spacing(10).into()
        };
        let top_bar = container(top_bar)
            .padding(if width == Width::Narrow { [10, 12] } else { [16, 24] })
            .width(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(Background::Color(t.surface)),
                border: Border { color: t.border, width: 0.0, radius: 0.0.into() },
                shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.2), offset: iced::Vector::new(0.0, 2.0), blur_radius: 10.0 },
                ..container::Style::default()
            });

        let body = container(body)
            .padding(if width == Width::Narrow { [6, 10] } else { [8, 24] })
            .width(Length::Fill)
            .height(Length::Fill);

        // The search popup floats over the page, right under the top bar;
        // clicking anywhere else on the page closes it.
        let body: Element<'_, Message> = if self.search_open {
            let results = search::results(&self.search, self.library_page.names().map(|(_, name)| name), self.settings.allow_mature);
            let popup = search::popup(results, content_width - 24.0, t);
            stack![
                body,
                mouse_area(container(Space::new()).width(Length::Fill).height(Length::Fill).style(|_theme| container::Style {
                    background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.25))),
                    ..container::Style::default()
                }))
                .on_press(Message::SearchClosed),
                container(opaque(popup)).width(Length::Fill).padding([8, 12]).align_x(Alignment::Center),
            ]
            .into()
        } else {
            body.into()
        };

        // Very short window: scroll the whole content area, giving the page
        // a usable minimum height instead of squashing it to nothing.
        const MIN_BODY_HEIGHT: f32 = 320.0;
        let content: Element<'_, Message> = if size.height < MIN_BODY_HEIGHT + 120.0 {
            scrollable(column![top_bar, container(body).height(Length::Fixed(MIN_BODY_HEIGHT))]).height(Length::Fill).into()
        } else {
            column![top_bar, body].width(Length::Fill).height(Length::Fill).into()
        };

        let mut layout = row![];
        if width != Width::Narrow {
            layout = layout.push(self.sidebar(width, rail_width, t));
        }
        container(layout.push(container(content).width(Length::Fill).height(Length::Fill)))
            .style(move |_theme| container::Style { background: Some(Background::Color(t.background)), ..container::Style::default() })
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn is_active(&self, page: Page) -> bool {
        match page {
            Page::Online => matches!(self.page, Page::Online | Page::Downloads),
            Page::Settings => matches!(self.page, Page::Settings | Page::Updates),
            other => self.page == other,
        }
    }

    fn has_badge(&self, page: Page) -> bool {
        page == Page::Settings && self.updates_page.available(&self.settings).is_some()
    }

    /// Full sidebar (icons + labels) or an icon-only rail; scrolls if the
    /// window is too short for every item.
    fn sidebar(&self, width: Width, rail_width: f32, t: theme::Tokens) -> Element<'_, Message> {
        let labels = width == Width::Wide;
        let mut items = column![].spacing(4);
        for (icon, label, page) in nav_pages() {
            items = items.push(nav_item(icon, label, page, self.is_active(page), self.has_badge(page), labels, t));
        }
        let brand: Element<'_, Message> = if labels {
            text("Paperloom").font(theme::DISPLAY_FONT).size(20).color(t.primary).wrapping(text::Wrapping::None).into()
        } else {
            container(text("P").font(theme::DISPLAY_FONT).size(22).color(t.primary)).center_x(Length::Fill).into()
        };

        container(scrollable(column![brand, items].spacing(20).padding(if labels { 16 } else { 8 })))
            .width(Length::Fixed(rail_width))
            .height(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(Background::Color(t.surface)),
                border: Border { color: t.border, width: 1.0, radius: 0.0.into() },
                shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.25), offset: iced::Vector::new(2.0, 0.0), blur_radius: 12.0 },
                ..container::Style::default()
            })
            .into()
    }
}

fn nav_pages() -> [(svg::Handle, &'static str, Page); 4] {
    [
        (icons::library(), "Library", Page::Library),
        (icons::globe(), "Online", Page::Online),
        (icons::layers(), "Sources", Page::Sources),
        (icons::settings(), "Settings", Page::Settings),
    ]
}

/// One navigation entry. `with_label`: icon + text (full sidebar), or icon
/// only (rail / top bar). `badge`: a small dot (Settings, when an update is
/// available).
fn nav_item<'a>(icon: svg::Handle, label: &'static str, page: Page, active: bool, badge: bool, with_label: bool, t: theme::Tokens) -> Element<'a, Message> {
    let icon_color = if active { t.primary } else { t.muted_foreground };
    let text_color = if active { t.primary } else { t.foreground };

    let mut content = row![svg(icon).width(18).height(18).style(move |_theme, _status| svg::Style { color: Some(icon_color) })]
        .spacing(10)
        .align_y(Alignment::Center);
    if with_label {
        content = content.push(text(label).font(theme::BODY_FONT).size(14).color(text_color).wrapping(text::Wrapping::None));
    }
    if badge {
        if with_label {
            content = content.push(Space::new().width(Length::Fill));
        }
        content = content.push(
            container(Space::new().width(8).height(8))
                .style(move |_theme| container::Style { background: Some(Background::Color(t.primary)), border: Border { radius: 4.0.into(), ..Border::default() }, ..container::Style::default() }),
        );
    }

    button(content)
        .padding([10, 12])
        .width(if with_label { Length::Fill } else { Length::Shrink })
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
