//! The share screens' looks: a wall of wallpaper cards on the left, the
//! controls in a panel on the right.

use iced::widget::{column, container, image, mouse_area, pick_list, row, scrollable, stack, text, text_input, Space};
use iced::{Alignment, Background, Border, Color, Element, Length};

use super::{ItemState, Message, State, QUALITIES};
use crate::theme;

pub const CODE_INPUT: &str = "share-code";
const CARD_WIDTH: f32 = 190.0;
const THUMB_HEIGHT: f32 = 107.0;

impl State {
    /// The screen to float over the library, if one is open. `collections`
    /// are the names that already exist.
    pub fn view<'a>(&'a self, collections: &[String], t: theme::Tokens) -> Option<Element<'a, Message>> {
        if let Some(sender) = &self.sender {
            return Some(self.sender_view(sender, t));
        }
        self.import.as_ref().map(|import| self.import_view(import, collections, t))
    }

    fn sender_view<'a>(&'a self, sender: &'a super::Sender, t: theme::Tokens) -> Element<'a, Message> {
        let count = sender.share.items.len();
        let left = sender.share.left_out;

        let wall: Element<'a, Message> = if count == 0 {
            container(text("None of these can be shared. They came from your own files, or they are hidden. Only wallpapers downloaded from a website can be sent.").size(14).color(t.muted_foreground))
                .center(Length::Fill)
                .padding(24)
                .into()
        } else {
            let cards = sender.share.items.iter().map(|item| {
                let poster = sender.posters.get(&item.key());
                card(poster.map(|p| p.as_path()), &item.t, &item.n, None, false, t)
            });
            scrollable(container(row(cards).spacing(12).wrap().vertical_spacing(12)).padding(iced::Padding { right: 14.0, ..iced::Padding::ZERO })).height(Length::Fill).into()
        };

        let mut side = column![
            text(format!("{count} wallpaper{}", if count == 1 { "" } else { "s" })).font(theme::DISPLAY_FONT).size(22).color(t.foreground),
            text("Your friend gets the same wallpapers from the same websites. Nothing from your computer is sent, only where each one comes from.").size(13).color(t.muted_foreground),
        ]
        .spacing(10);
        if left > 0 {
            side = side.push(text(format!("{left} left out: they came from your own files, or they are hidden.")).size(13).color(t.muted_foreground));
        }
        if count > 0 {
            if Self::code_is_long(&sender.share) {
                side = side.push(text("That is a lot for a chat message. A file is easier to send.").size(13).color(t.muted_foreground));
            }
            side = side.push(Space::new().height(8));
            side = side.push(
                iced::widget::button(crate::ui::label("Copy share code").center()).style(crate::ui::primary).padding([10, 16]).width(Length::Fill).on_press(Message::CopyCode),
            );
            side = side.push(
                iced::widget::button(crate::ui::label("Save as a file…").center()).style(crate::ui::secondary).padding([10, 16]).width(Length::Fill).on_press(Message::SaveFile),
            );
        }
        if let Some(notice) = &sender.notice {
            side = side.push(text(notice.clone()).size(13).color(t.primary));
        }
        let title = format!("Share {}", sender.title);
        crate::ui::screen(title, None, row![wall, panel(side.into(), t)].spacing(20).height(Length::Fill).into(), Message::CloseSender, t)
    }

    fn import_view<'a>(&'a self, import: &'a super::Import, collections: &[String], t: theme::Tokens) -> Element<'a, Message> {
        let wall: Element<'a, Message> = match &import.share {
            None => container(
                column![
                    text("Paste a share code, or open a share file").font(theme::DISPLAY_FONT).size(20).color(t.foreground),
                    text("You can also drop a .paperloomshare file anywhere on the window.").size(13).color(t.muted_foreground),
                ]
                .spacing(8)
                .align_x(Alignment::Center),
            )
            .center(Length::Fill)
            .into(),
            Some(share) => {
                let cards = share.items.iter().enumerate().map(|(index, item)| {
                    let state = import.states.get(index).copied().unwrap_or(ItemState::New);
                    let note = match state {
                        ItemState::New => format!("{}{}", item.n, if item.has_4k() { " · HD, 4K" } else { "" }),
                        ItemState::InLibrary => "Already in your library".to_string(),
                        ItemState::Queued => "Already downloading".to_string(),
                    };
                    let picked = import.picked.get(index).copied().unwrap_or(false);
                    let thumb = self.thumbs.get(&item.key());
                    let tile = card(thumb.map(|p| p.as_path()), &item.t, &note, Some(picked), state != ItemState::New, t);
                    if state == ItemState::New {
                        mouse_area(tile).on_press(Message::Toggle(index, !picked)).into()
                    } else {
                        tile
                    }
                });
                scrollable(container(row(cards).spacing(12).wrap().vertical_spacing(12)).padding(iced::Padding { right: 14.0, ..iced::Padding::ZERO })).height(Length::Fill).into()
            }
        };

        let mut side = column![
            text("Share code").size(13).color(t.muted_foreground),
            text_input("Paste the code here", &import.text).id(CODE_INPUT).on_input(Message::Text).padding(8).width(Length::Fill),
            row![
                iced::widget::button(crate::ui::label("Paste").center()).style(crate::ui::secondary).padding([8, 14]).width(Length::Fill).on_press(Message::Paste),
                iced::widget::button(crate::ui::label("Open file…").center()).style(crate::ui::secondary).padding([8, 14]).width(Length::Fill).on_press(Message::ChooseFile),
            ]
            .spacing(8),
        ]
        .spacing(8);
        if let Some(error) = &import.error {
            side = side.push(text(error.clone()).size(13).color(t.destructive));
        }

        if let Some(share) = &import.share {
            let total = share.items.len();
            let ticked = import.picked.iter().filter(|p| **p).count();
            let owned = import.states.iter().filter(|s| **s == ItemState::InLibrary).count();
            side = side.push(crate::ui::rule(t));
            side = side.push(text(if share.name.is_empty() { format!("{total} wallpapers") } else { share.name.clone() }).font(theme::DISPLAY_FONT).size(20).color(t.foreground));
            let mut facts = format!("{total} wallpaper{}", if total == 1 { "" } else { "s" });
            if owned > 0 {
                facts.push_str(&format!(" · you have {owned}"));
            }
            if share.left_out > 0 {
                facts.push_str(&format!(" · {} not included", share.left_out));
            }
            side = side.push(text(facts).size(12).color(t.muted_foreground));
            side = side.push(
                row![
                    iced::widget::button(crate::ui::label("Select all").size(12)).style(crate::ui::ghost).padding([4, 8]).on_press(Message::PickAll(true)),
                    iced::widget::button(crate::ui::label("Select none").size(12)).style(crate::ui::ghost).padding([4, 8]).on_press(Message::PickAll(false)),
                ]
                .spacing(4),
            );
            side = side.push(
                row![text("Quality").size(13).color(t.foreground), pick_list(QUALITIES, Some(import.quality), Message::Quality)].spacing(8).align_y(Alignment::Center),
            );
            let exists = collections.iter().any(|name| name.eq_ignore_ascii_case(import.collection.trim()));
            side = side.push(text("Collection (leave empty for none)").size(13).color(t.foreground));
            side = side.push(text_input("Collection name", &import.collection).on_input(Message::Collection).padding(8));
            side = side.push(
                text(if import.collection.trim().is_empty() {
                    String::new()
                } else if exists {
                    "You already have one with this name. They will be added to it.".to_string()
                } else {
                    "A new collection will be created.".to_string()
                })
                .size(12)
                .color(t.muted_foreground),
            );
            side = side.push(Space::new().height(Length::Fill));
            let has_name = !import.collection.trim().is_empty();
            let (label, message) = if ticked > 0 {
                (format!("Download {ticked}"), Some(Message::Start))
            } else if owned > 0 && has_name {
                ("Add to collection".to_string(), Some(Message::Start))
            } else {
                ("Download".to_string(), None)
            };
            side = side.push(iced::widget::button(crate::ui::label(label).center()).style(crate::ui::primary).padding([10, 16]).width(Length::Fill).on_press_maybe(message));
        }
        crate::ui::screen("Import a share", None, row![wall, panel(side.into(), t)].spacing(20).height(Length::Fill).into(), Message::CloseImport, t)
    }
}

/// The right-hand column: a fixed width, with a hairline on its left.
fn panel<'a>(content: Element<'a, Message>, t: theme::Tokens) -> Element<'a, Message> {
    container(container(content).padding(iced::Padding { left: 20.0, ..iced::Padding::ZERO }).height(Length::Fill))
        .width(Length::Fixed(320.0))
        .height(Length::Fill)
        .style(move |_theme| container::Style { border: Border { color: t.border, width: 0.0, radius: 0.0.into() }, ..container::Style::default() })
        .into()
}

/// One wallpaper: its picture, its name and a line of detail. `picked`:
/// `Some` when it can be ticked; `dim` when it can't be chosen.
fn card<'a>(thumb: Option<&std::path::Path>, title: &str, note: &str, picked: Option<bool>, dim: bool, t: theme::Tokens) -> Element<'a, Message> {
    let picture: Element<'a, Message> = match thumb {
        Some(path) => image(image::Handle::from_path(path)).content_fit(iced::ContentFit::Cover).width(Length::Fill).height(Length::Fill).into(),
        None => container(iced::widget::svg(crate::icons::film()).width(24).height(24).style(move |_theme, _status| iced::widget::svg::Style { color: Some(t.muted_foreground) }))
            .center(Length::Fill)
            .style(move |_theme| container::Style { background: Some(Background::Color(t.muted)), ..container::Style::default() })
            .into(),
    };
    let mut layers = stack![container(picture).width(Length::Fill).height(Length::Fixed(THUMB_HEIGHT)).clip(true)];
    if let Some(picked) = picked {
        if !dim {
            layers = layers.push(container(crate::ui::check_chip(picked, t)).padding(6).width(Length::Fill).align_x(Alignment::End));
        }
    }
    let selected = picked == Some(true);
    let text_color = if dim { t.muted_foreground } else { t.foreground };
    container(column![layers, column![text(title.to_string()).size(13).color(text_color).wrapping(text::Wrapping::None), text(note.to_string()).size(11).color(t.muted_foreground).wrapping(text::Wrapping::None)].spacing(2).padding([8, 8])])
        .width(Length::Fixed(CARD_WIDTH))
        .clip(true)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(t.surface)),
            border: Border { color: if selected { t.primary } else { t.border }, width: if selected { 2.0 } else { 1.0 }, radius: 6.0.into() },
            text_color: Some(Color::WHITE),
            ..container::Style::default()
        })
        .into()
}
