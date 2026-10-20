use std::collections::HashSet;
use std::time::Duration;

use iced::widget::{button, column, container, mouse_area, opaque, responsive, row, scrollable, stack, svg, text, text_input, Space};
use iced::futures::StreamExt;
use iced::{Alignment, Background, Border, Color, Element, Length, Shadow, Size, Subscription, Task, Theme};

use paperloom_common::Settings;

use crate::pages::{downloads, library, online, settings, sources, updates, usage};
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
    usage_page: usage::State,
    /// Short usage history so the graphs are already filled when opened.
    recorder: crate::telemetry::Recorder,
    /// Which sidebar groups are open (the tree is collapsible).
    expanded: HashSet<Group>,
    /// Sources changed since the Online tab last loaded; reload it when
    /// next opened so the change takes effect.
    online_stale: bool,
    /// What the desktop says its light/dark preference is; only consulted
    /// when the user's theme setting is "System".
    system_theme: iced::theme::Mode,
    /// The global search box (top bar) and whether its popup is showing.
    search: String,
    search_open: bool,
    /// A newer Paperloom was installed over this one (by `apt upgrade` or
    /// the Updates page); the banner offers a restart.
    replaced: bool,
    banner_dismissed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Page {
    Library,
    Online,
    Downloads,
    Sources,
    Settings,
    Updates,
    Usage,
}

/// A collapsible group in the sidebar tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Group {
    Library,
    Collections,
    Online,
    Settings,
}

/// Where a sidebar row leads: a page, or one of the Library's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NavTarget {
    Page(Page),
    Tab(library::Tab),
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
    Usage(usage::Message),
    /// Another launch of Paperloom (tray click, app menu…) asked this
    /// window for something: "SHOW", "PAGE usage", "TRAY-SETTINGS".
    OtherLaunch(String),
    ToggleGroup(Group),
    Navigate(NavTarget),
    /// Fires while downloads are in flight: moves finished files into the
    /// library.
    DownloadsTick,
    Record,
    SystemTheme(iced::theme::Mode),
    // --- Global search (see `search.rs`)
    SearchChanged(String),
    /// Enter in the search box: search online for what was typed.
    SearchSubmitted,
    SearchClosed,
    SearchOnline(String),
    GoToSetting(settings::SettingId),
    ShowInLibrary(String),
    // --- "New version installed" banner
    CheckReplaced,
    Restart,
    BannerDismissed,
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
            usage_page: usage::State::new(),
            recorder: crate::telemetry::Recorder::new(),
            expanded: HashSet::from([Group::Library, Group::Online, Group::Settings]),
            online_stale: false,
            system_theme: iced::theme::Mode::None,
            search: String::new(),
            search_open: false,
            replaced: false,
            banner_dismissed: false,
        };
        // `--page usage` etc. (from the tray menu).
        let requested_page = std::env::args().skip_while(|arg| arg != "--page").nth(1).as_deref().and_then(page_named);
        if let Some(page) = requested_page {
            app.page = page;
            if page == Page::Usage {
                app.usage_page.open(app.library_page.library());
            }
        }
        // Opened from the tray menu's "Tray icon settings…".
        let start = if std::env::args().any(|arg| arg == "--tray-settings") {
            app.page = Page::Settings;
            app.settings_page.refresh(app.library_page.library());
            app.settings_page.update(&mut app.settings, settings::Message::ResultSelected(settings::SettingId::TrayIcon)).map(Message::Settings)
        } else {
            Task::none()
        };
        let check = if app.settings.updates.check_on_startup {
            app.updates_page.start_check(&app.settings).map(Message::Updates)
        } else {
            Task::none()
        };
        // Fill the Online page now (from saved results when they're fresh),
        // so it is already full by the time the tab is opened.
        let warm = app.online_page.ensure_loaded(&app.settings).map(Message::Online);
        (app, Task::batch([iced::system::theme().map(Message::SystemTheme), check, start, warm]))
    }

    fn effective_theme(&self) -> paperloom_common::ThemeMode {
        theme::resolve(self.settings.theme, self.system_theme)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::CheckReplaced => {
                self.replaced = paperloom_common::process::exe_replaced();
                Task::none()
            }
            Message::Restart | Message::Updates(updates::Message::Restart) => crate::engine_client::restart_app(),
            Message::BannerDismissed => {
                self.banner_dismissed = true;
                Task::none()
            }
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
                self.settings_page.refresh(self.library_page.library());
                self.settings_page.update(&mut self.settings, settings::Message::ResultSelected(id)).map(Message::Settings)
            }
            Message::ShowInLibrary(name) => {
                self.search_open = false;
                self.page = Page::Library;
                self.library_page.set_filter(name);
                Task::none()
            }
            Message::OtherLaunch(request) => {
                let mut tasks = vec![raise_window()];
                if request == "TRAY-SETTINGS" {
                    self.page = Page::Settings;
                    self.settings_page.refresh(self.library_page.library());
                    tasks.push(
                        self.settings_page
                            .update(&mut self.settings, settings::Message::ResultSelected(settings::SettingId::TrayIcon))
                            .map(Message::Settings),
                    );
                } else if let Some(page) = request.strip_prefix("PAGE ") {
                    if let Some(page) = page_named(page) {
                        tasks.push(self.update(Message::SwitchPage(page)));
                    }
                }
                Task::batch(tasks)
            }
            Message::ToggleGroup(group) => {
                if !self.expanded.remove(&group) {
                    self.expanded.insert(group);
                }
                Task::none()
            }
            Message::Navigate(NavTarget::Page(page)) => self.update(Message::SwitchPage(page)),
            Message::Navigate(NavTarget::Tab(tab)) => {
                self.search_open = false;
                self.page = Page::Library;
                self.expanded.insert(Group::Library);
                self.library_page.update(&mut self.settings, library::Message::TabSelected(tab)).map(Message::Library)
            }
            Message::Usage(usage::Message::Back) => {
                self.page = Page::Settings;
                self.settings_page.refresh(self.library_page.library());
                Task::none()
            }
            Message::Usage(msg) => self.usage_page.update(msg).map(Message::Usage),
            Message::Settings(settings::Message::OpenUsage) => self.update(Message::SwitchPage(Page::Usage)),
            Message::SwitchPage(page) => {
                self.search_open = false;
                self.page = page;
                // Opening a page opens its group, so the tree shows where you are.
                match page {
                    Page::Online | Page::Downloads | Page::Sources => {
                        self.expanded.insert(Group::Online);
                    }
                    Page::Settings | Page::Updates | Page::Usage => {
                        self.expanded.insert(Group::Settings);
                    }
                    Page::Library => {
                        self.expanded.insert(Group::Library);
                    }
                }
                if page == Page::Usage {
                    self.usage_page.open(self.library_page.library());
                }
                if page == Page::Settings {
                    self.settings_page.refresh(self.library_page.library());
                }
                if page == Page::Downloads {
                    self.downloads_page.set_hosts(crate::pages::network::hosts_for(&self.settings));
                    self.downloads_page.seed(&self.recorder.history.network);
                    return self.downloads_page.opened().map(Message::Downloads);
                }
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
                self.settings_page.refresh(self.library_page.library());
                Task::none()
            }
            Message::Updates(msg) => self.updates_page.update(&mut self.settings, msg).map(Message::Updates),
            Message::Library(msg) => self.library_page.update(&mut self.settings, msg).map(Message::Library),
            Message::Online(msg) => self.online_page.update(&self.settings, msg).map(Message::Online),
            Message::Downloads(downloads::Message::Back) => {
                self.page = Page::Online;
                Task::none()
            }
            Message::Downloads(msg) => {
                self.downloads_page.set_hosts(crate::pages::network::hosts_for(&self.settings));
                self.downloads_page.update(self.online_page.queue(), msg).map(Message::Downloads)
            }
            Message::Sources(msg) => {
                self.online_stale = true;
                self.sources_page.update(&mut self.settings, msg).map(Message::Sources)
            }
            Message::SystemTheme(mode) => {
                self.system_theme = mode;
                Task::none()
            }
            Message::Record => {
                self.recorder.tick();
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
            self.usage_page.subscription(self.page == Page::Usage).map(Message::Usage),
            iced::time::every(crate::telemetry::EVERY).map(|_| Message::Record),
            Subscription::run(|| crate::instance::requests().map(Message::OtherLaunch)),
            if self.online_page.queue().is_busy() || !self.online_page.queue().finished().is_empty() || self.page == Page::Downloads {
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
            if cfg!(target_os = "linux") && !self.replaced {
                // A `readlink` every few seconds: notices `apt upgrade`.
                iced::time::every(Duration::from_secs(5)).map(|_| Message::CheckReplaced)
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
            Page::Library => ("Library", self.library_page.header().map(Message::Library), self.library_page.body(self.settings.library_view, t).map(Message::Library)),
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
            Page::Usage => ("Usage", self.usage_page.header().map(Message::Usage), self.usage_page.body(&self.settings, t, &self.recorder.history).map(Message::Usage)),
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
            if let Some(update) = self.update_button(t) {
                nav = nav.push(update);
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
            let mut bar = row![title_text, search_box, header].spacing(12).align_y(Alignment::Center);
            if let Some(update) = self.update_button(t) {
                bar = bar.push(update);
            }
            bar.push(self.downloads_button(t)).wrap().vertical_spacing(10).into()
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
            let results = search::results(&self.search, self.library_page.names(), self.settings.allow_mature);
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
        let top_bar: Element<'_, Message> = match self.restart_banner(t) {
            Some(banner) => column![banner, top_bar].into(),
            None => top_bar.into(),
        };
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

    /// "New version installed — restart" across the top of the window.
    fn restart_banner(&self, t: theme::Tokens) -> Option<Element<'_, Message>> {
        let installed = self.updates_page.installed();
        if self.banner_dismissed || (installed.is_none() && !self.replaced) {
            return None;
        }
        let message = match installed {
            Some(version) => format!("Paperloom {version} is installed. Restart to start using it."),
            None => "A new version of Paperloom was installed. Restart to start using it.".to_string(),
        };
        let on_primary = t.background;
        Some(
            container(
                row![
                    text(message).size(14).color(on_primary).width(Length::Fill),
                    button(crate::ui::label("Restart now").size(13))
                        .padding([5, 12])
                        .style(move |_theme, status| button::Style {
                            background: Some(Background::Color(if matches!(status, button::Status::Hovered | button::Status::Pressed) { t.surface } else { t.background })),
                            text_color: t.foreground,
                            border: Border { radius: 6.0.into(), ..Border::default() },
                            shadow: Shadow::default(),
                            snap: false,
                        })
                        .on_press(Message::Restart),
                    button(crate::ui::label("Later").size(13))
                        .padding([5, 10])
                        .style(move |_theme, _status| button::Style { text_color: on_primary, ..button::Style::default() })
                        .on_press(Message::BannerDismissed),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding([8, 16])
            .width(Length::Fill)
            .style(move |_theme| container::Style { background: Some(Background::Color(t.primary)), ..container::Style::default() })
            .into(),
        )
    }

    fn is_active(&self, page: Page) -> bool {
        match page {
            Page::Online => matches!(self.page, Page::Online | Page::Downloads | Page::Sources),
            Page::Settings => matches!(self.page, Page::Settings | Page::Updates | Page::Usage),
            other => self.page == other,
        }
    }

    fn has_badge(&self, page: Page) -> bool {
        page == Page::Settings && self.updates_page.available(&self.settings).is_some()
    }

    /// "Update 0.7.0" in the top bar whenever a newer release is known, on
    /// every page, so it's one click from anywhere.
    fn update_button(&self, t: theme::Tokens) -> Option<Element<'_, Message>> {
        let release = self.updates_page.available(&self.settings)?;
        Some(
            button(crate::ui::label(format!("Update {}", release.tag.trim_start_matches('v'))).size(13))
                .padding([7, 12])
                .on_press(Message::SwitchPage(Page::Updates))
                .style(move |_theme, status| button::Style {
                    background: Some(Background::Color(if matches!(status, button::Status::Hovered | button::Status::Pressed) { t.primary_hover } else { t.primary })),
                    text_color: t.background,
                    border: Border { radius: 8.0.into(), ..Border::default() },
                    shadow: Shadow::default(),
                    snap: false,
                })
                .into(),
        )
    }

    /// One row of the sidebar tree: a small icon and a label, indented by
    /// `depth`. `badge` is a count or marker shown at the right.
    #[allow(clippy::too_many_arguments)]
    fn tree_row<'a>(&self, icon: svg::Handle, label: String, depth: u16, active: bool, badge: Option<String>, on_press: Message, t: theme::Tokens) -> Element<'a, Message> {
        let color = if active { t.primary } else { t.foreground };
        let icon_color = if active { t.primary } else { t.muted_foreground };
        let mut content = row![
            svg(icon).width(15).height(15).style(move |_theme, _status| svg::Style { color: Some(icon_color) }),
            text(label).font(theme::BODY_FONT).size(13).color(color).wrapping(text::Wrapping::None),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        if let Some(badge) = badge {
            content = content.push(Space::new().width(Length::Fill));
            content = content.push(text(badge).size(11).color(t.primary));
        }
        let _ = depth;
        button(content)
            .padding([6, 10])
            .width(Length::Fill)
            .on_press(on_press)
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
                    border: Border { radius: 7.0.into(), ..Border::default() },
                    shadow: Shadow::default(),
                    snap: false,
                }
            })
            .into()
    }

    /// A group header: chevron (expand/collapse) + icon + label (opens the
    /// group's main page).
    #[allow(clippy::too_many_arguments)]
    fn group_header<'a>(&self, icon: svg::Handle, label: &'static str, group: Group, open: Message, active: bool, badge: Option<String>, t: theme::Tokens) -> Element<'a, Message> {
        let expanded = self.expanded.contains(&group);
        let color = if active { t.primary } else { t.muted_foreground };
        let chevron = button(svg(if expanded { icons::chevron_down() } else { icons::chevron_right() }).width(14).height(14).style(move |_theme, _status| svg::Style { color: Some(t.muted_foreground) }))
            .padding([9, 6])
            .on_press(Message::ToggleGroup(group))
            .style(|_theme, _status| button::Style::default());
        let label_color = if active { t.primary } else { t.foreground };
        let mut line = row![svg(icon).width(18).height(18).style(move |_theme, _status| svg::Style { color: Some(color) }), text(label).font(theme::BODY_FONT).size(14).color(label_color).wrapping(text::Wrapping::None)]
            .spacing(10)
            .align_y(Alignment::Center);
        if let Some(badge) = badge {
            line = line.push(Space::new().width(Length::Fill));
            line = line.push(text(badge).size(11).color(t.primary));
        }
        let opener = button(line).padding([9, 6]).width(Length::Fill).on_press(open).style(move |_theme, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: if hovered { Some(Background::Color(t.muted)) } else { None },
                text_color: label_color,
                border: Border { radius: 7.0.into(), ..Border::default() },
                shadow: Shadow::default(),
                snap: false,
            }
        });
        row![chevron, opener].align_y(Alignment::Center).into()
    }

    /// Children of a group, indented. (No height-filling guide line: a
    /// "fill" height inside nested groups made the layout collapse when a
    /// group had nothing in it.)
    fn children<'a>(&self, rows: Vec<Element<'a, Message>>, depth: u16, _t: theme::Tokens) -> Element<'a, Message> {
        row![Space::new().width(Length::Fixed(14.0 + f32::from(depth) * 12.0)), column(rows).spacing(1).width(Length::Fill)].into()
    }

    /// The whole tree: Library (All / Collections → each collection /
    /// Hidden), Online (Browse / Downloads / Sources) and Settings
    /// (Overview / Usage / Updates), each group collapsible.
    fn tree<'a>(&self, t: theme::Tokens) -> Element<'a, Message> {
        let tab = self.library_page.tab();
        let on_library = self.page == Page::Library;
        let go_tab = |tab: library::Tab| Message::Navigate(NavTarget::Tab(tab));
        let go_page = |page: Page| Message::Navigate(NavTarget::Page(page));
        let mut tree = column![].spacing(2);

        // --- Library
        tree = tree.push(self.group_header(icons::library(), "Library", Group::Library, go_tab(library::Tab::All), on_library, None, t));
        if self.expanded.contains(&Group::Library) {
            let mut rows = vec![self.tree_row(icons::grid(), "All wallpapers".into(), 1, on_library && tab == library::Tab::All, None, go_tab(library::Tab::All), t)];

            let collections = &self.library_page.library().collections;
            let open = self.expanded.contains(&Group::Collections);
            rows.push(
                row![
                    button(svg(if open { icons::chevron_down() } else { icons::chevron_right() }).width(12).height(12).style(move |_theme, _status| svg::Style { color: Some(t.muted_foreground) }))
                        .padding([6, 4])
                        .on_press(Message::ToggleGroup(Group::Collections))
                        .style(|_theme, _status| button::Style::default()),
                    self.tree_row(
                        icons::folder(),
                        "Collections".into(),
                        1,
                        on_library && tab == library::Tab::Collections,
                        (!collections.is_empty()).then(|| collections.len().to_string()),
                        go_tab(library::Tab::Collections),
                        t,
                    ),
                ]
                .align_y(Alignment::Center)
                .into(),
            );
            if open {
                // An empty group says so (and stays open) instead of
                // looking like it did nothing.
                let mut inner: Vec<Element<'a, Message>> = collections
                    .iter()
                    .filter(|collection| !collection.hidden)
                    .map(|collection| {
                        self.tree_row(
                            icons::folder(),
                            collection.name.clone(),
                            2,
                            on_library && tab == library::Tab::Collection(collection.id),
                            None,
                            go_tab(library::Tab::Collection(collection.id)),
                            t,
                        )
                    })
                    .collect();
                if inner.is_empty() {
                    inner.push(container(text("No collections here yet").size(12).color(t.muted_foreground)).padding([4, 10]).into());
                }
                // However many collections there are, this list scrolls in
                // place (no visible bar) instead of pushing the rest of the
                // sidebar off the bottom.
                let list = scrollable(column(inner).spacing(1).width(Length::Fill))
                    .direction(iced::widget::scrollable::Direction::Vertical(iced::widget::scrollable::Scrollbar::hidden()));
                rows.push(row![Space::new().width(Length::Fixed(26.0)), container(list).max_height(220.0).width(Length::Fill)].into());
            }
            rows.push(self.tree_row(icons::eye_off(), "Hidden".into(), 1, on_library && tab == library::Tab::Hidden, None, go_tab(library::Tab::Hidden), t));
            tree = tree.push(self.children(rows, 0, t));
        }

        // --- Online
        let pending = self.online_page.queue().snapshot().iter().filter(|job| job.status != paperloom_fetch::Status::Done).count();
        tree = tree.push(self.group_header(
            icons::globe(),
            "Online",
            Group::Online,
            go_page(Page::Online),
            matches!(self.page, Page::Online | Page::Downloads | Page::Sources),
            None,
            t,
        ));
        if self.expanded.contains(&Group::Online) {
            let rows = vec![
                self.tree_row(icons::compass(), "Browse".into(), 1, self.page == Page::Online, None, go_page(Page::Online), t),
                self.tree_row(icons::download(), "Downloads".into(), 1, self.page == Page::Downloads, (pending > 0).then(|| pending.to_string()), go_page(Page::Downloads), t),
                self.tree_row(icons::layers(), "Sources".into(), 1, self.page == Page::Sources, None, go_page(Page::Sources), t),
            ];
            tree = tree.push(self.children(rows, 0, t));
        }

        // --- Settings
        let update = self.updates_page.available(&self.settings).is_some();
        tree = tree.push(self.group_header(
            icons::settings(),
            "Settings",
            Group::Settings,
            go_page(Page::Settings),
            matches!(self.page, Page::Settings | Page::Updates | Page::Usage),
            None,
            t,
        ));
        if self.expanded.contains(&Group::Settings) {
            let rows = vec![
                self.tree_row(icons::dashboard(), "Overview".into(), 1, self.page == Page::Settings, None, go_page(Page::Settings), t),
                self.tree_row(icons::activity(), "Usage".into(), 1, self.page == Page::Usage, None, go_page(Page::Usage), t),
                self.tree_row(icons::refresh(), "Updates".into(), 1, self.page == Page::Updates, update.then(|| "new".to_string()), go_page(Page::Updates), t),
            ];
            tree = tree.push(self.children(rows, 0, t));
        }
        tree.into()
    }

    /// Full sidebar (icons + labels) or an icon-only rail; scrolls if the
    /// window is too short for every item.
    fn sidebar(&self, width: Width, rail_width: f32, t: theme::Tokens) -> Element<'_, Message> {
        let labels = width == Width::Wide;
        let items: Element<'_, Message> = if labels {
            self.tree(t)
        } else {
            let mut rail = column![].spacing(4);
            for (icon, label, page) in nav_pages() {
                rail = rail.push(nav_item(icon, label, page, self.is_active(page), self.has_badge(page), false, t));
            }
            rail.into()
        };
        let brand: Element<'_, Message> = if labels {
            text("Paperloom").font(theme::DISPLAY_FONT).size(20).color(t.primary).wrapping(text::Wrapping::None).into()
        } else {
            container(text("P").font(theme::DISPLAY_FONT).size(22).color(t.primary)).center_x(Length::Fill).into()
        };

        // Scrolls when the tree outgrows the window, with no visible bar.
        container(
            scrollable(column![brand, items].spacing(20).padding(if labels { 16 } else { 8 }))
                .direction(iced::widget::scrollable::Direction::Vertical(iced::widget::scrollable::Scrollbar::hidden())),
        )
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

/// The rail / top-bar navigation: one icon per sidebar group (their
/// sub-pages are reached from inside the pages, or from the full sidebar).
fn nav_pages() -> [(svg::Handle, &'static str, Page); 3] {
    [(icons::library(), "Library", Page::Library), (icons::globe(), "Online", Page::Online), (icons::settings(), "Settings", Page::Settings)]
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

/// Page for a `--page <name>` argument or a "PAGE <name>" request.
fn page_named(name: &str) -> Option<Page> {
    match name.trim().to_lowercase().as_str() {
        "library" => Some(Page::Library),
        "online" => Some(Page::Online),
        "downloads" => Some(Page::Downloads),
        "sources" => Some(Page::Sources),
        "settings" => Some(Page::Settings),
        "updates" => Some(Page::Updates),
        "usage" => Some(Page::Usage),
        _ => None,
    }
}

/// Brings the window to the front and gives it focus — un-minimising it
/// first. A window manager can refuse a plain focus request from a window
/// that wasn't just used, so it also takes a quick turn as "always on top",
/// which reliably lifts it above everything else.
fn raise_window() -> Task<Message> {
    iced::window::oldest().and_then(|id| {
        Task::batch([
            iced::window::minimize(id, false),
            iced::window::set_level(id, iced::window::Level::AlwaysOnTop),
            iced::window::gain_focus(id),
            iced::window::set_level(id, iced::window::Level::Normal),
        ])
    })
}
