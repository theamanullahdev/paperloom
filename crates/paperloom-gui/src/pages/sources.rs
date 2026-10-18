//! Sources: which online sites the Online tab searches, their API keys, and
//! user-added URL sources. Everything lives in `Settings::sources` and is
//! saved on every change; the Online tab reads it fresh each time it
//! loads, so changes apply to the next search.

use iced::widget::{button, column, container, row, text, text_input, toggler, Space};
use iced::{Background, Border, Element, Length, Task};

use paperloom_common::{Settings, SourceSetting};

use crate::theme;

#[derive(Default)]
pub struct State {
    new_name: String,
    new_url: String,
    new_key: String,
}

#[derive(Debug, Clone)]
pub enum Message {
    Toggle(String, bool),
    KeyChanged(String, String),
    UrlChanged(String, String),
    Remove(String),
    NewName(String),
    NewUrl(String),
    NewKey(String),
    Add,
}

impl State {
    pub fn update(&mut self, settings: &mut Settings, message: Message) -> Task<Message> {
        match message {
            Message::Toggle(id, enabled) => edit(settings, &id, |source| source.enabled = enabled),
            Message::KeyChanged(id, key) => edit(settings, &id, |source| source.api_key = key),
            Message::UrlChanged(id, url) => edit(settings, &id, |source| source.url = url),
            Message::Remove(id) => {
                settings.sources.retain(|source| source.builtin || source.id != id);
                let _ = paperloom_common::save(settings);
            }
            Message::NewName(value) => self.new_name = value,
            Message::NewUrl(value) => self.new_url = value,
            Message::NewKey(value) => self.new_key = value,
            Message::Add => {
                let (name, url) = (self.new_name.trim(), self.new_url.trim());
                if !name.is_empty() && !url.is_empty() {
                    let next = settings
                        .sources
                        .iter()
                        .filter_map(|source| source.id.strip_prefix("custom-")?.parse::<u32>().ok())
                        .max()
                        .map_or(1, |highest| highest + 1);
                    settings.sources.push(SourceSetting {
                        id: format!("custom-{next}"),
                        name: name.to_string(),
                        enabled: true,
                        api_key: self.new_key.trim().to_string(),
                        url: url.to_string(),
                        builtin: false,
                    });
                    let _ = paperloom_common::save(settings);
                    *self = State::default();
                }
            }
        }
        Task::none()
    }

    pub fn body<'a>(&'a self, settings: &'a Settings, t: theme::Tokens) -> Element<'a, Message> {
        let mut list = column![
            text("The Online tab searches every source that's switched on, all at once. API keys are free from each site and are saved on this computer only (in your Paperloom config file, readable just by you).")
                .size(13)
                .color(t.muted_foreground),
        ]
        .spacing(12);

        for source in &settings.sources {
            list = list.push(source_card(source, t));
        }

        let can_add = !self.new_name.trim().is_empty() && !self.new_url.trim().is_empty();
        let add = column![
            text("Add your own source").size(15).color(t.foreground),
            text("A web page or JSON API URL that lists links to MP4 files. Use {q} where the search text should go (optional). Paging isn't supported for custom sources.")
                .size(12)
                .color(t.muted_foreground),
            text_input("Name", &self.new_name).on_input(Message::NewName).padding(8),
            text_input("URL, e.g. https://example.com/videos?search={q}", &self.new_url).on_input(Message::NewUrl).padding(8),
            text_input("API key (optional)", &self.new_key).on_input(Message::NewKey).secure(true).padding(8),
            row![
                Space::new().width(Length::Fill),
                button(crate::ui::label("Add source")).style(button::primary).on_press_maybe(can_add.then_some(Message::Add)),
            ],
        ]
        .spacing(8);
        list = list.push(card(add.into(), t));

        crate::ui::scroll_page(list).into()
    }
}

fn edit(settings: &mut Settings, id: &str, change: impl FnOnce(&mut SourceSetting)) {
    if let Some(source) = settings.sources.iter_mut().find(|source| source.id == id) {
        change(source);
        let _ = paperloom_common::save(settings);
    }
}

fn source_card<'a>(source: &'a SourceSetting, t: theme::Tokens) -> Element<'a, Message> {
    let (status, status_color) = if source.needs_key() && source.api_key.trim().is_empty() {
        ("Needs an API key", t.primary_hover)
    } else if source.enabled {
        ("Active", t.success)
    } else {
        ("Off", t.muted_foreground)
    };

    let mut content = column![row![
        toggler(source.enabled).label(source.name.clone()).on_toggle({
            let id = source.id.clone();
            move |enabled| Message::Toggle(id.clone(), enabled)
        }),
        Space::new().width(Length::Fill),
        text(status).size(12).color(status_color),
    ]
    .align_y(iced::Alignment::Center)]
    .spacing(8);

    let hint = match source.id.as_str() {
        paperloom_common::SOURCE_MOTIONBGS => Some("motionbgs.com — no key needed. Offers HD and 4K."),
        paperloom_common::SOURCE_PEXELS => Some("Free key at pexels.com/api. Offers HD and 4K."),
        paperloom_common::SOURCE_PIXABAY => Some("Free key at pixabay.com/api/docs. Offers HD and 4K where available."),
        _ => None,
    };
    if let Some(hint) = hint {
        content = content.push(text(hint).size(12).color(t.muted_foreground));
    }

    if !source.builtin {
        let id = source.id.clone();
        content = content.push(text_input("URL", &source.url).on_input({
            let id = id.clone();
            move |value| Message::UrlChanged(id.clone(), value)
        }).padding(8));
    }

    if source.needs_key() || !source.builtin {
        let id = source.id.clone();
        let placeholder = if source.needs_key() { "API key (required)" } else { "API key (optional)" };
        content = content.push(
            text_input(placeholder, &source.api_key)
                .on_input(move |value| Message::KeyChanged(id.clone(), value))
                .secure(true)
                .padding(8),
        );
    }

    if !source.builtin {
        content = content.push(row![
            Space::new().width(Length::Fill),
            button(crate::ui::label("Remove").size(12)).style(button::danger).padding([4, 10]).on_press(Message::Remove(source.id.clone())),
        ]);
    }

    card(content.into(), t)
}

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
