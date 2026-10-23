//! Picking several wallpapers at once in the library: a "Select" mode with
//! ticks on the tiles and a bar of things to do with the pick (share, add to
//! a collection, hide, remove).

use std::collections::HashSet;

use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Background, Border, Color, Element, Length};
use paperloom_common::Library;

use crate::theme;

#[derive(Debug, Clone)]
pub enum Message {
    Begin,
    End,
    Toggle(u64),
    SelectAll,
    ShareIt,
    HideIt,
    AskCollection,
    ToCollection(u64),
    NewName(String),
    CreateAndAdd,
    AskRemove,
    ConfirmRemove,
    CloseDialog,
}

enum Dialog {
    Collection { new_name: String },
    Remove,
}

/// What the library page should do after a selection message.
pub enum Outcome {
    Nothing,
    /// Wallpapers were hidden, removed or moved: redraw from scratch.
    Changed(Option<String>),
    Share(Vec<u64>),
    SelectAll,
}

#[derive(Default)]
pub struct Selection {
    active: bool,
    picked: HashSet<u64>,
    dialog: Option<Dialog>,
}

impl Selection {
    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn is_picked(&self, id: u64) -> bool {
        self.picked.contains(&id)
    }

    pub fn pick_all(&mut self, ids: impl IntoIterator<Item = u64>) {
        self.picked = ids.into_iter().collect();
    }

    /// Leaves selecting (when the user changes tab, say).
    pub fn reset(&mut self) {
        *self = Selection::default();
    }

    pub fn update(&mut self, message: Message, library: &mut Library) -> Outcome {
        match message {
            Message::Begin => self.active = true,
            Message::End => self.reset(),
            Message::Toggle(id) => {
                if !self.picked.remove(&id) {
                    self.picked.insert(id);
                }
            }
            Message::SelectAll => return Outcome::SelectAll,
            Message::ShareIt => return Outcome::Share(self.sorted()),
            Message::HideIt => {
                let ids = self.sorted();
                let mut note = None;
                for id in &ids {
                    if let Err(err) = library.set_hidden(*id, true) {
                        note = Some(format!("Couldn't save the library: {err}"));
                    }
                }
                self.reset();
                return Outcome::Changed(note);
            }
            Message::AskCollection => self.dialog = Some(Dialog::Collection { new_name: String::new() }),
            Message::ToCollection(collection) => {
                let note = self.add_to(library, collection);
                return Outcome::Changed(note);
            }
            Message::NewName(name) => {
                if let Some(Dialog::Collection { new_name }) = &mut self.dialog {
                    *new_name = name;
                }
            }
            Message::CreateAndAdd => {
                if let Some(Dialog::Collection { new_name }) = &self.dialog {
                    let name = new_name.trim().to_string();
                    if !name.is_empty() {
                        return match library.create_collection(&name) {
                            Ok(collection) => Outcome::Changed(self.add_to(library, collection)),
                            Err(err) => Outcome::Changed(Some(format!("Couldn't save the library: {err}"))),
                        };
                    }
                }
            }
            Message::AskRemove => self.dialog = Some(Dialog::Remove),
            Message::ConfirmRemove => {
                let ids = self.sorted();
                let mut note = None;
                for id in &ids {
                    if let Err(err) = library.remove(*id) {
                        note = Some(format!("Couldn't remove a wallpaper: {err}"));
                    }
                }
                self.reset();
                return Outcome::Changed(note);
            }
            Message::CloseDialog => self.dialog = None,
        }
        Outcome::Nothing
    }

    /// Closes the add/remove pop-up; `false` when none was open.
    pub fn close_dialog(&mut self) -> bool {
        self.dialog.take().is_some()
    }

    fn sorted(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self.picked.iter().copied().collect();
        ids.sort_unstable();
        ids
    }

    fn add_to(&mut self, library: &mut Library, collection: u64) -> Option<String> {
        let mut note = None;
        for id in self.sorted() {
            if let Err(err) = library.set_in_collection(collection, id, true) {
                note = Some(format!("Couldn't save the library: {err}"));
            }
        }
        self.reset();
        note
    }

    /// The bar above the wallpapers while selecting.
    pub fn strip<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let count = self.picked.len();
        let some = count > 0;
        let act = |label: &'static str, message: Message| button(crate::ui::label(label).size(13)).style(crate::ui::secondary).padding([6, 12]).on_press_maybe(some.then_some(message));
        let strip = row![
            text(if some { format!("{count} selected") } else { "Click wallpapers to select them".to_string() }).size(14).color(t.foreground).width(Length::Fixed(230.0)),
            button(crate::ui::label("Select all").size(13)).style(crate::ui::ghost).padding([6, 12]).on_press(Message::SelectAll),
            act("Share", Message::ShareIt),
            act("Add to collection", Message::AskCollection),
            act("Hide", Message::HideIt),
            button(crate::ui::label("Remove").size(13)).style(crate::ui::danger).padding([6, 12]).on_press_maybe(some.then_some(Message::AskRemove)),
            button(crate::ui::label("Done").size(13)).style(crate::ui::primary).padding([6, 14]).on_press(Message::End),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .wrap()
        .vertical_spacing(6);
        container(strip)
            .padding([10, 14])
            .width(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(Background::Color(Color { a: 0.12, ..t.primary })),
                border: Border { color: Color { a: 0.55, ..t.primary }, width: 1.0, radius: 6.0.into() },
                ..container::Style::default()
            })
            .into()
    }

    /// The "add to collection" or "remove" pop-up, when one is open.
    pub fn dialog<'a>(&'a self, library: &'a Library, t: theme::Tokens) -> Option<Element<'a, Message>> {
        let count = self.picked.len();
        let plural = if count == 1 { "" } else { "s" };
        let content: Element<'a, Message> = match self.dialog.as_ref()? {
            Dialog::Remove => column![
                text(format!("Remove {count} wallpaper{plural}?")).font(theme::DISPLAY_FONT).size(18).color(t.foreground),
                text("They leave Paperloom. Copies kept in the library folder are deleted; files you imported in place are left where they are.").size(13).color(t.muted_foreground),
                row![
                    button(crate::ui::label("Remove")).style(crate::ui::danger).on_press(Message::ConfirmRemove),
                    button(crate::ui::label("Cancel").size(12)).style(crate::ui::ghost).on_press(Message::CloseDialog),
                ]
                .spacing(8),
            ]
            .spacing(10)
            .into(),
            Dialog::Collection { new_name } => {
                let list: Element<'a, Message> = if library.collections.is_empty() {
                    text("No collections yet. Name one below.").size(12).color(t.muted_foreground).into()
                } else {
                    scrollable(
                        column(library.collections.iter().map(|collection| {
                            button(
                                row![
                                    crate::ui::label(collection.name.clone()).size(14).width(Length::Fill),
                                    text(format!("{}", collection.members.len())).size(12).color(t.muted_foreground),
                                ]
                                .align_y(Alignment::Center),
                            )
                            .style(crate::ui::secondary)
                            .padding([10, 14])
                            .width(Length::Fill)
                            .on_press(Message::ToCollection(collection.id))
                            .into()
                        }))
                        .spacing(6)
                        .padding(iced::Padding { right: 12.0, ..iced::Padding::ZERO }),
                    )
                    .height(Length::Fixed((library.collections.len() as f32 * 46.0).min(230.0)))
                    .into()
                };
                column![
                    text(format!("Add {count} wallpaper{plural} to a collection")).font(theme::DISPLAY_FONT).size(18).color(t.foreground),
                    list,
                    row![
                        text_input("New collection…", new_name).on_input(Message::NewName).on_submit(Message::CreateAndAdd).padding(6).width(Length::Fill),
                        button(crate::ui::label("Create")).style(crate::ui::secondary).on_press_maybe((!new_name.trim().is_empty()).then_some(Message::CreateAndAdd)),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                    button(crate::ui::label("Cancel").size(12)).style(crate::ui::ghost).on_press(Message::CloseDialog),
                ]
                .spacing(10)
                .into()
            }
        };
        Some(crate::ui::modal(content, 380.0, Message::CloseDialog, t))
    }
}
