//! The one search box in the top bar. Typing opens a popup that answers
//! "where can this take me?" in three clearly labelled groups:
//!
//! - **Search online**: the typed words, plus related keywords, as
//!   wallpaper searches (Online page, all sources at once).
//! - **Pages & settings**: pages by name/alias, and individual settings via
//!   the settings registry's aliases ("dark mode" finds Theme).
//! - **Your library**: wallpapers already imported, by name.
//!
//! Enter searches online. Escape closes it.

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Alignment, Background, Border, Color, Element, Length, Shadow};

use crate::app::{Message, Page};
use crate::pages::settings::{self, SettingId};
use crate::theme;

/// Pages and the words people might use for them.
const PAGES: &[(Page, &str, &[&str])] = &[
    (Page::Library, "Library", &["library", "my wallpapers", "imported", "import", "videos"]),
    (Page::Online, "Online", &["online", "browse", "explore", "download wallpapers", "store", "latest"]),
    (Page::Downloads, "Downloads", &["downloads", "queue", "progress", "downloading"]),
    (Page::Sources, "Sources", &["sources", "sites", "api key", "pexels", "pixabay", "motionbgs", "custom source"]),
    (Page::Settings, "Settings", &["settings", "preferences", "options", "config"]),
    (Page::Updates, "Updates", &["updates", "update", "upgrade", "new version", "release", "version"]),
];

const MAX_SETTINGS: usize = 6;
const MAX_WALLPAPERS: usize = 5;
const RELATED: usize = 6;

pub struct Results<'a> {
    query: String,
    pages: Vec<(Page, &'static str)>,
    settings: Vec<&'static settings::Entry>,
    wallpapers: Vec<&'a str>,
    related: Vec<String>,
    /// The query is 18+ and the setting is off.
    mature_blocked: bool,
}

pub fn results<'a>(query: &str, wallpapers: impl Iterator<Item = &'a str>, allow_mature: bool) -> Results<'a> {
    let query = query.trim().to_string();
    let lower = query.to_lowercase();
    let pages = PAGES
        .iter()
        .filter(|(_, label, aliases)| label.to_lowercase().contains(&lower) || aliases.iter().any(|alias| alias.contains(&lower) || lower.contains(alias)))
        .map(|(page, label, _)| (*page, *label))
        .collect();
    let settings = settings::ENTRIES.iter().filter(|entry| settings::matches(entry, &query)).take(MAX_SETTINGS).collect();
    let wallpapers = wallpapers.filter(|name| name.to_lowercase().contains(&lower)).take(MAX_WALLPAPERS).collect();
    let mature_blocked = !allow_mature && paperloom_fetch::keywords::is_mature(&query);
    let related = if mature_blocked { Vec::new() } else { paperloom_fetch::keywords::related_queries(&query, RELATED) };
    Results { query, pages, settings, wallpapers, related, mature_blocked }
}

/// The popup under the search box. `width` is the space available.
pub fn popup<'a>(results: Results<'a>, width: f32, t: theme::Tokens) -> Element<'a, Message> {
    let mut list = column![].spacing(4);

    // --- Search online
    list = list.push(heading("Search online wallpapers", t));
    if results.mature_blocked {
        list = list.push(
            column![
                text("That search is 18+, which is turned off.").size(13).color(t.foreground),
                button(crate::ui::label("Change in Settings → Content").size(12))
                    .style(crate::ui::secondary)
                    .padding([4, 10])
                    .on_press(Message::GoToSetting(SettingId::AllowMature)),
            ]
            .spacing(6),
        );
    } else {
        list = list.push(result_row(
            format!("Search for “{}”", results.query),
            "Online · all sources, plus related keywords".to_string(),
            Message::SearchOnline(results.query.clone()),
            true,
            t,
        ));
        if !results.related.is_empty() {
            let mut chips = row![text("Related:").size(12).color(t.muted_foreground)].spacing(6).align_y(Alignment::Center);
            for keyword in &results.related {
                chips = chips.push(
                    button(crate::ui::label(keyword.clone()).size(12))
                        .style(crate::ui::secondary)
                        .padding([3, 9])
                        .on_press(Message::SearchOnline(keyword.clone())),
                );
            }
            list = list.push(container(chips.wrap().vertical_spacing(6)).padding([4, 8]));
        }
    }

    // --- Pages & settings
    if !results.pages.is_empty() || !results.settings.is_empty() {
        list = list.push(Space::new().height(6)).push(heading("Pages & settings", t));
        for (page, label) in &results.pages {
            list = list.push(result_row(label.to_string(), "Page".to_string(), Message::SwitchPage(*page), false, t));
        }
        for entry in &results.settings {
            list = list.push(result_row(entry.label.to_string(), format!("Setting · {}", entry.section), Message::GoToSetting(entry.id), false, t));
        }
    }

    // --- Your library
    if !results.wallpapers.is_empty() {
        list = list.push(Space::new().height(6)).push(heading("Your library", t));
        for name in &results.wallpapers {
            list = list.push(result_row(name.to_string(), "Wallpaper in your library".to_string(), Message::ShowInLibrary(name.to_string()), false, t));
        }
    }

    list = list.push(Space::new().height(4)).push(
        text("Enter searches online · Esc closes").size(11).color(t.muted_foreground),
    );

    container(scrollable(container(list).padding(10)))
        .width(Length::Fixed(width.clamp(120.0, 560.0)))
        .max_height(440.0)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(t.surface)),
            border: Border { color: t.border, width: 1.0, radius: 6.0.into() },
            shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.35), offset: iced::Vector::new(0.0, 6.0), blur_radius: 24.0 },
            ..container::Style::default()
        })
        .into()
}

fn heading<'a>(label: &'static str, t: theme::Tokens) -> Element<'a, Message> {
    container(text(label).size(12).color(t.muted_foreground)).padding([4, 8]).into()
}

/// One clickable result: title plus a muted line saying what it is.
fn result_row<'a>(title: String, kind: String, message: Message, primary: bool, t: theme::Tokens) -> Element<'a, Message> {
    button(
        column![
            text(title).size(14).color(if primary { t.primary } else { t.foreground }),
            text(kind).size(11).color(t.muted_foreground),
        ]
        .spacing(2),
    )
    .width(Length::Fill)
    .padding([6, 8])
    .on_press(message)
    .style(move |_theme, status| button::Style {
        background: matches!(status, button::Status::Hovered | button::Status::Pressed).then_some(Background::Color(t.muted)),
        text_color: t.foreground,
        border: Border { radius: 4.0.into(), ..Border::default() },
        shadow: Shadow::default(),
        snap: false,
    })
    .into()
}
