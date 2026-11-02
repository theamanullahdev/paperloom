//! Looks: one place for every look, in three tabs. On this computer is what
//! ships with Paperloom plus what was installed from its online library;
//! Online library is the catalogue; Imported is what the user brought in from
//! Rainmeter or a file, grouped into packs that can be opened, removed one by
//! one, or all at once (a Rainmeter collection can be hundreds of looks).
//!
//! Every look card has the same three buttons in the same places: Use, Add,
//! Remove (empty for the built-in ones).

use std::collections::BTreeMap;

use iced::widget::{button, column, container, row, text, Space};
use iced::{Alignment, Background, Border, Element, Length};

use paperloom_common::widgets::BUILT_IN;
use paperloom_skin::Skin;

use super::{card, BrowseTab, Message, State};
use crate::pages::widgets_looks;
use crate::theme;
use crate::ui;

/// How many looks of an open pack are drawn at a time.
pub(super) const PAGE: usize = 24;

/// What a removal is waiting for the user to confirm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirm {
    /// Every look of this pack.
    Pack(String),
    /// Every imported look.
    AllImported,
}

impl State {
    /// The looks of one pack, or every imported look when `pack` is `None`.
    fn imported_looks(&self, pack: Option<&str>) -> Vec<&'static Skin> {
        paperloom_skin::all().into_iter().filter(|skin| skin.imported() && pack.is_none_or(|name| skin.pack_name() == name)).collect()
    }

    /// The imported looks by pack, in name order.
    fn packs(&self) -> BTreeMap<String, Vec<&'static Skin>> {
        let mut packs: BTreeMap<String, Vec<&'static Skin>> = BTreeMap::new();
        for skin in self.imported_looks(None) {
            packs.entry(skin.pack_name()).or_default().push(skin);
        }
        packs
    }

    /// How many widgets on the desktop use one of these looks.
    fn in_use_count(&self, skins: &[&Skin]) -> usize {
        self.widgets.placed.iter().filter(|placed| skins.iter().any(|skin| skin.id == placed.skin)).count()
    }

    /// The previews the open pack needs drawn (the ones shown now).
    pub(super) fn pack_ids(&self) -> Vec<String> {
        match &self.pack_open {
            Some(name) => self.imported_looks(Some(name)).into_iter().take(self.pack_shown).map(|skin| skin.id.clone()).collect(),
            None => Vec::new(),
        }
    }

    /// Looks: the gallery of looks, each drawn on the user's wallpaper.
    pub(super) fn looks<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let imported = self.imported_looks(None).len();
        let tabs = ui::segmented(
            vec![
                ("On this computer".to_string(), self.tab == BrowseTab::Installed, Message::Tab(BrowseTab::Installed)),
                ("Online library".to_string(), self.tab == BrowseTab::Online, Message::Tab(BrowseTab::Online)),
                (if imported == 0 { "Imported".to_string() } else { format!("Imported ({imported})") }, self.tab == BrowseTab::Imported, Message::Tab(BrowseTab::Imported)),
            ],
            t,
        );
        match self.tab {
            BrowseTab::Imported => column![tabs, self.imported_tab(t)].spacing(14).into(),
            BrowseTab::Online => {
                let target = |kinds: &[String], skin: &str| self.target_for(kinds).map(|placed| (self.title_of(placed), widgets_looks::skin_of(placed).is_some_and(|current| current.id == skin)));
                column![
                    tabs,
                    text("More looks, some with their own fonts and images. Install one, then use it on a widget you have or add it as a new one.").size(13).color(t.muted_foreground),
                    self.kind_filters(t),
                    self.online.view(&self.filter, &target, t).map(Message::Online),
                ]
                .spacing(14)
                .into()
            }
            BrowseTab::Installed => {
                let cards: Vec<Element<'a, Message>> = paperloom_skin::all()
                    .into_iter()
                    .filter(|skin| !skin.imported() && (self.filter.is_empty() || skin.kinds.contains(&self.filter)))
                    .map(|skin| self.look_card(skin, t))
                    .collect();
                let hint = "Use changes the widget you already have. Add puts a new one on the desktop.";
                column![
                    tabs,
                    row![
                        text(hint).size(13).color(t.muted_foreground).width(Length::Fill),
                        button(ui::label("Import Rainmeter skins…")).style(ui::ghost).on_press(Message::Tab(BrowseTab::Imported)),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                    self.kind_filters(t),
                    row(cards).spacing(16).wrap().vertical_spacing(16),
                ]
                .spacing(14)
                .into()
            }
        }
    }

    fn kind_filters<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let kinds = std::iter::once((String::new(), "All".to_string()))
            .chain(BUILT_IN.iter().filter(|entry| !paperloom_skin::for_kind(entry.kind).is_empty()).map(|entry| (entry.kind.to_string(), entry.name.to_string())));
        ui::segmented(kinds.map(|(kind, name)| (name, self.filter == kind, Message::Filter(kind))).collect(), t)
    }

    /// The Imported tab: how to bring more in, and the packs that are here.
    fn imported_tab<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let packs = self.packs();
        let total: usize = packs.values().map(Vec::len).sum();
        let mut buttons = row![].spacing(8).align_y(Alignment::Center);
        if self.importing {
            buttons = buttons.push(text("Importing…").size(13).color(t.muted_foreground));
        } else {
            buttons = buttons
                .push(button(ui::label("Import a file…").color(ui::on_accent())).style(ui::primary).on_press(Message::ImportRainmeter(false)))
                .push(button(ui::label("Import a folder…")).style(ui::secondary).on_press(Message::ImportRainmeter(true)));
            if total > 0 {
                buttons = buttons.push(button(ui::label("Delete all")).style(ui::danger).on_press(Message::AskRemoveImported));
            }
        }
        let summary = match (total, packs.len()) {
            (0, _) => "Nothing imported yet. Bring in a Rainmeter skin package (.rmskin), a skin folder or an .ini file; each skin becomes a look here.".to_string(),
            (1, _) => "1 look, in 1 pack.".to_string(),
            (n, 1) => format!("{n} looks, in 1 pack."),
            (n, p) => format!("{n} looks, in {p} packs."),
        };
        let mut page = column![
            row![
                column![text("Imported looks").size(16).color(t.foreground), text(summary).size(13).color(t.muted_foreground)].spacing(2).width(Length::Fill),
                buttons,
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        ]
        .spacing(14);
        for (name, skins) in packs {
            page = page.push(self.pack_row(name, skins, t));
        }
        page.into()
    }

    /// One pack: a line that opens it, and Remove pack.
    fn pack_row<'a>(&'a self, name: String, skins: Vec<&'static Skin>, t: theme::Tokens) -> Element<'a, Message> {
        let open = self.pack_open.as_deref() == Some(name.as_str());
        let count = if skins.len() == 1 { "1 look".to_string() } else { format!("{} looks", skins.len()) };
        let used = self.in_use_count(&skins);
        let detail = if used == 0 { count } else { format!("{count} · {used} on your desktop") };
        let line = row![
            button(
                row![
                    text(if open { "▾" } else { "▸" }).size(14).color(t.muted_foreground).width(Length::Fixed(16.0)),
                    text(name.clone()).size(15).color(t.foreground),
                    text(detail).size(13).color(t.muted_foreground),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding([6, 4])
            .width(Length::Fill)
            .style(move |_theme, status| button::Style {
                background: matches!(status, button::Status::Hovered).then_some(Background::Color(t.muted)),
                text_color: t.foreground,
                border: Border { radius: 6.0.into(), ..Border::default() },
                ..button::Style::default()
            })
            .on_press(Message::OpenPack(name.clone())),
            button(ui::label("Remove pack")).style(ui::ghost).on_press(Message::AskRemovePack(name.clone())),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let mut body = column![line].spacing(12);
        if open {
            let shown = self.pack_shown.min(skins.len());
            let cards: Vec<Element<'a, Message>> = skins.iter().take(shown).map(|skin| self.look_card(skin, t)).collect();
            body = body.push(row(cards).spacing(16).wrap().vertical_spacing(16));
            if shown < skins.len() {
                body = body.push(button(ui::label(format!("Show {} more", PAGE.min(skins.len() - shown)))).style(ui::secondary).on_press(Message::MorePack));
            }
        }
        container(body)
            .padding(10)
            .width(Length::Fill)
            .style(move |_theme| container::Style { background: Some(Background::Color(t.surface)), border: Border { color: t.border, width: 1.0, radius: 8.0.into() }, ..container::Style::default() })
            .into()
    }

    /// The widget "Use" changes for a look of these kinds: the one picked last
    /// if it can show it, else the first that can.
    pub(super) fn target_for(&self, kinds: &[String]) -> Option<&paperloom_common::widgets::Placed> {
        let selected = self.selected.and_then(|id| self.widgets.placed.iter().find(|placed| placed.id == id)).filter(|placed| kinds.contains(&placed.kind));
        selected.or_else(|| self.widgets.placed.iter().find(|placed| kinds.contains(&placed.kind)))
    }

    /// A look's card. The buttons are always Use, Add and Remove, in that order and place.
    fn look_card<'a>(&'a self, skin: &'static Skin, t: theme::Tokens) -> Element<'a, Message> {
        let target = self.target_for(&skin.kinds);
        // The kind it is shown as: the filter's, else its widget's, else its first.
        let kind = if skin.kinds.contains(&self.filter) {
            self.filter.clone()
        } else {
            target.map(|placed| placed.kind.clone()).or_else(|| skin.kinds.first().cloned()).unwrap_or_default()
        };
        let in_use = target.is_some_and(|placed| widgets_looks::skin_of(placed).is_some_and(|current| current.id == skin.id));
        // `None` makes it a quiet label (nothing to press), so the others never move.
        let slot = |label: &'static str, message: Option<Message>, style: fn(&iced::Theme, button::Status) -> button::Style, filled: bool| -> Element<'a, Message> {
            let mut name = ui::label(label);
            if filled {
                name = name.color(ui::on_accent());
            }
            let face = container(name).center_x(Length::Fill);
            match message {
                Some(message) => button(face).width(Length::Fill).style(style).on_press(message).into(),
                None => button(face).width(Length::Fill).style(ui::ghost).into(),
            }
        };
        let use_slot = if in_use {
            slot("In use", None, ui::ghost, false)
        } else {
            match target {
                Some(_) => slot("Use", Some(Message::UseLook(skin.id.clone())), ui::primary, true),
                None => slot("Use", None, ui::ghost, false),
            }
        };
        let remove_slot: Element<'a, Message> = match skin.folder() {
            Some(folder) => slot("Remove", Some(Message::RemoveLook(folder.to_string())), ui::ghost, false),
            None => Space::new().width(Length::Fill).into(),
        };
        let actions = vec![use_slot, slot("Add", Some(Message::AddWithLook(kind.clone(), skin.id.clone())), ui::secondary, false), remove_slot];
        let detail = match target.filter(|_| in_use) {
            Some(placed) => format!("In use on your {}", self.title_of(placed)),
            None => format!("by {}", skin.author),
        };
        card(self.previews.card(&skin.id, &kind), skin.name.clone(), detail, actions, None, in_use, t)
    }

    /// The question before a removal of many looks, and the pop-up about an import.
    pub(super) fn looks_overlay<'a>(&'a self, t: theme::Tokens) -> Option<Element<'a, Message>> {
        if let Some(confirm) = &self.confirm {
            let (title, what, count) = match confirm {
                Confirm::Pack(name) => {
                    let skins = self.imported_looks(Some(name));
                    (format!("Remove the pack “{name}”?"), format!("its {}", if skins.len() == 1 { "1 look".to_string() } else { format!("{} looks", skins.len()) }), self.in_use_count(&skins))
                }
                Confirm::AllImported => {
                    let skins = self.imported_looks(None);
                    ("Delete every imported look?".to_string(), format!("all {} imported looks", skins.len()), self.in_use_count(&skins))
                }
            };
            let mut body = column![text(title).size(18).color(t.foreground), text(format!("This takes {what} off this computer.")).size(13).color(t.muted_foreground)].spacing(8);
            if count > 0 {
                let which = if count == 1 { "1 widget on your desktop uses one".to_string() } else { format!("{count} widgets on your desktop use them") };
                body = body.push(text(format!("{which}; it goes back to its default look.")).size(13).color(t.foreground));
            }
            body = body.push(Space::new().height(Length::Fixed(6.0))).push(
                row![
                    Space::new().width(Length::Fill),
                    button(ui::label("Cancel")).style(ui::ghost).on_press(Message::CancelRemove),
                    button(ui::label("Remove")).style(ui::danger).on_press(Message::ConfirmRemove),
                ]
                .spacing(8),
            );
            return Some(ui::modal(body, 420.0, Message::CancelRemove, t));
        }
        if !self.show_import {
            return None;
        }
        let body: Element<'a, Message> = if self.importing {
            column![
                text("Importing…").size(18).color(t.foreground),
                text("Reading the skins and copying their pictures and fonts. A big pack can take a minute.").size(13).color(t.muted_foreground),
            ]
            .spacing(8)
            .into()
        } else {
            match &self.imported {
                Some(Ok((looks, left_out))) => {
                    let names = looks.iter().take(5).cloned().collect::<Vec<_>>().join(", ") + &if looks.len() > 5 { format!(" and {} more", looks.len() - 5) } else { String::new() };
                    let mut body = column![
                        text(if looks.len() == 1 { "Added 1 look".to_string() } else { format!("Added {} looks", looks.len()) }).size(18).color(t.success),
                        text(names).size(13).color(t.muted_foreground),
                    ]
                    .spacing(8);
                    if !left_out.is_empty() {
                        body = body.push(text(format!("{} parts of them could not be carried over, so those look simpler than in Rainmeter.", left_out.len())).size(13).color(t.muted_foreground));
                    }
                    body.push(Space::new().height(Length::Fixed(6.0)))
                        .push(
                            row![
                                Space::new().width(Length::Fill),
                                button(ui::label("Close")).style(ui::ghost).on_press(Message::CloseImport),
                                button(ui::label("See them").color(ui::on_accent())).style(ui::primary).on_press(Message::SeeImported),
                            ]
                            .spacing(8),
                        )
                        .into()
                }
                Some(Err(err)) => column![
                    text("Nothing was imported").size(18).color(t.destructive),
                    text(err.as_str()).size(13).color(t.muted_foreground),
                    Space::new().height(Length::Fixed(6.0)),
                    row![Space::new().width(Length::Fill), button(ui::label("Close")).style(ui::secondary).on_press(Message::CloseImport)],
                ]
                .spacing(8)
                .into(),
                None => return None,
            }
        };
        Some(ui::modal(body, 440.0, Message::CloseImport, t))
    }
}
