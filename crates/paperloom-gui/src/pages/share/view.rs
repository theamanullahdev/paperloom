//! The share dialogs' looks.

use iced::widget::{checkbox, column, container, image, pick_list, row, scrollable, text, text_input};
use iced::{Alignment, Background, Border, Element, Length};

use super::{ItemState, Message, State, QUALITIES};
use crate::theme;

impl State {
    /// The dialog to float over the library, if one is open. `collections`
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
        let mut body = column![text(format!("Share {}", sender.title)).font(theme::DISPLAY_FONT).size(18).color(t.foreground)].spacing(10);
        if count == 0 {
            body = body.push(text("None of these can be shared. They came from your own files, or they are hidden. Only wallpapers downloaded from a website can be sent.").size(13).color(t.muted_foreground));
        } else {
            body = body.push(text(format!("{count} wallpaper{} in this share.", if count == 1 { "" } else { "s" })).size(13).color(t.foreground));
            if left > 0 {
                body = body.push(text(format!("{left} left out: they came from your own files, or they are hidden.")).size(12).color(t.muted_foreground));
            }
            body = body.push(
                text("Your friend gets the same wallpapers from the same websites. Nothing from your computer is sent.").size(12).color(t.muted_foreground),
            );
            if Self::code_is_long(&sender.share) {
                body = body.push(text("That is a lot for a chat message. A file is easier to send.").size(12).color(t.muted_foreground));
            }
            body = body.push(
                row![
                    button_primary("Copy share code", Message::CopyCode),
                    button_secondary("Save as a file…", Message::SaveFile),
                ]
                .spacing(8)
                .wrap()
                .vertical_spacing(6),
            );
        }
        if let Some(notice) = &sender.notice {
            body = body.push(text(notice.clone()).size(12).color(t.primary));
        }
        body = body.push(button_ghost("Close", Message::CloseSender));
        crate::ui::modal(body.spacing(10), 420.0, Message::CloseSender, t)
    }

    fn import_view<'a>(&'a self, import: &'a super::Import, collections: &[String], t: theme::Tokens) -> Element<'a, Message> {
        let mut body = column![
            text("Import a share").font(theme::DISPLAY_FONT).size(18).color(t.foreground),
            text("Paste a share code a friend sent you, or open a share file.").size(13).color(t.muted_foreground),
            row![
                text_input("Paste the share code here", &import.text).on_input(Message::Text).padding(6).width(Length::Fill),
                button_secondary("Paste", Message::Paste),
                button_secondary("Open file…", Message::ChooseFile),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(10);
        if let Some(error) = &import.error {
            body = body.push(text(error.clone()).size(13).color(t.destructive));
        }
        if let Some(share) = &import.share {
            let total = share.items.len();
            let ticked = import.picked.iter().filter(|p| **p).count();
            let owned = import.states.iter().filter(|s| **s == ItemState::InLibrary).count();
            let heading = if share.name.is_empty() { format!("{total} wallpapers") } else { format!("{} · {total} wallpapers", share.name) };
            body = body.push(text(heading).size(15).color(t.foreground));
            if share.left_out > 0 {
                body = body.push(text(format!("{} more couldn't be included.", share.left_out)).size(12).color(t.muted_foreground));
            }
            if owned > 0 {
                body = body.push(text(format!("You already have {owned} of these.")).size(12).color(t.muted_foreground));
            }
            body = body.push(
                row![
                    button_ghost("Select all", Message::PickAll(true)),
                    button_ghost("Select none", Message::PickAll(false)),
                ]
                .spacing(6),
            );
            let list = column(share.items.iter().enumerate().map(|(index, item)| {
                let state = import.states.get(index).copied().unwrap_or(ItemState::New);
                let thumb: Element<'a, Message> = match self.thumbs.get(&item.key()) {
                    Some(path) => container(image(image::Handle::from_path(path)).content_fit(iced::ContentFit::Cover).width(Length::Fill).height(Length::Fill))
                        .width(Length::Fixed(64.0))
                        .height(Length::Fixed(36.0))
                        .clip(true)
                        .into(),
                    None => container(text("")).width(Length::Fixed(64.0)).height(Length::Fixed(36.0)).style(move |_theme| container::Style {
                        background: Some(Background::Color(t.muted)),
                        border: Border { radius: 4.0.into(), ..Border::default() },
                        ..container::Style::default()
                    })
                    .into(),
                };
                let note = match state {
                    ItemState::New => format!("{}{}", item.n, if item.has_4k() { " · HD and 4K" } else { "" }),
                    ItemState::InLibrary => "Already in your library".to_string(),
                    ItemState::Queued => "Already downloading".to_string(),
                };
                let enabled = state == ItemState::New;
                let mut tick = checkbox(import.picked.get(index).copied().unwrap_or(false));
                if enabled {
                    tick = tick.on_toggle(move |on| Message::Toggle(index, on));
                }
                row![
                    tick,
                    thumb,
                    column![text(item.t.clone()).size(13).color(t.foreground).wrapping(text::Wrapping::None), text(note).size(11).color(t.muted_foreground)].spacing(2).width(Length::Fill),
                ]
                .spacing(10)
                .align_y(Alignment::Center)
                .into()
            }))
            .spacing(8)
            .padding(iced::Padding { right: 12.0, ..iced::Padding::ZERO });
            body = body.push(container(scrollable(list)).max_height(176.0));

            let exists = collections.iter().any(|name| name.eq_ignore_ascii_case(import.collection.trim()));
            body = body.push(
                row![
                    text("Quality").size(13).color(t.foreground),
                    pick_list(QUALITIES, Some(import.quality), Message::Quality),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
            body = body.push(
                column![
                    text("Put them in a collection (leave empty for none)").size(13).color(t.foreground),
                    text_input("Collection name", &import.collection).on_input(Message::Collection).padding(6),
                    text(if import.collection.trim().is_empty() {
                        String::new()
                    } else if exists {
                        "You already have a collection with this name. They will be added to it.".to_string()
                    } else {
                        "A new collection will be created.".to_string()
                    })
                    .size(11)
                    .color(t.muted_foreground),
                ]
                .spacing(4),
            );
            let has_name = !import.collection.trim().is_empty();
            let go = if ticked > 0 {
                button_primary_maybe(&format!("Download {ticked}"), Some(Message::Start))
            } else if owned > 0 && has_name {
                button_primary_maybe("Add to collection", Some(Message::Start))
            } else {
                button_primary_maybe("Download", None)
            };
            body = body.push(row![go, button_ghost("Cancel", Message::CloseImport)].spacing(8));
        } else {
            body = body.push(button_ghost("Cancel", Message::CloseImport));
        }
        crate::ui::modal(body, 520.0, Message::CloseImport, t)
    }
}

fn button_primary<'a>(label: &str, message: Message) -> Element<'a, Message> {
    button_primary_maybe(label, Some(message))
}

fn button_primary_maybe<'a>(label: &str, message: Option<Message>) -> Element<'a, Message> {
    iced::widget::button(crate::ui::label(label.to_string())).style(crate::ui::primary).on_press_maybe(message).into()
}

fn button_secondary<'a>(label: &str, message: Message) -> Element<'a, Message> {
    iced::widget::button(crate::ui::label(label.to_string())).style(crate::ui::secondary).on_press(message).into()
}

fn button_ghost<'a>(label: &str, message: Message) -> Element<'a, Message> {
    iced::widget::button(crate::ui::label(label.to_string()).size(12)).style(crate::ui::ghost).on_press(message).into()
}

