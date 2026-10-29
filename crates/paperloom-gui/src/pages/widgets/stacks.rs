//! Stacks on the Widgets pages: the Stacks page (ready-made sets to put on the
//! desktop, installed or from the online library), a stack's card on My
//! widgets, and a stack's own page (where it sits, its size, what is in it).
//! They use the same cards as everything else here.

use iced::widget::{button, column, container, row, slider, text, Space};
use iced::{Alignment, Background, Border, Element, Length};

use paperloom_common::stacks::{Preset, Stack};

use super::{card, section, BrowseTab, Message, State};
use crate::theme;
use crate::ui;

impl State {
    /// How many stacks of this preset are on the desktop.
    fn on_desktop(&self, preset: &str) -> usize {
        self.widgets.stacks.iter().filter(|stack| stack.preset == preset).count()
    }

    /// The Stacks page.
    pub(super) fn stacks_page<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let tabs = ui::segmented(
            vec![
                ("On this computer".to_string(), self.stacks_tab == BrowseTab::Installed, Message::StacksTab(BrowseTab::Installed)),
                ("Online library".to_string(), self.stacks_tab == BrowseTab::Online, Message::StacksTab(BrowseTab::Online)),
            ],
            t,
        );
        if self.stacks_tab == BrowseTab::Online {
            return column![
                tabs,
                text("Stacks other people made. Installing one also downloads the looks it uses. Then add it to your desktop from its card.").size(13).color(t.muted_foreground),
                self.stacks_online.view("", &|_, _| None, t).map(Message::StackOnline),
            ]
            .spacing(14)
            .into();
        }
        let cards: Vec<Element<'a, Message>> = paperloom_common::stacks::presets().into_iter().map(|preset| self.preset_card(preset, t)).collect();
        column![
            tabs,
            text("A stack is a ready-made set of widgets: add it once and it lands on your desktop as a group. Drag any part of it and the whole stack moves.").size(13).color(t.muted_foreground),
            iced::widget::row(cards).spacing(16).wrap().vertical_spacing(16),
        ]
        .spacing(14)
        .into()
    }

    fn preset_card<'a>(&'a self, preset: Preset, t: theme::Tokens) -> Element<'a, Message> {
        let count = self.on_desktop(&preset.id);
        // Already on the desktop: taking it off is right here, next to adding another.
        let mut actions: Vec<Element<'a, Message>> = Vec::new();
        if count == 0 {
            actions.push(button(ui::label("Add to desktop").color(ui::on_accent())).style(ui::primary).on_press(Message::AddStack(preset.id.clone())).into());
        } else {
            let label = if count == 1 { "Remove stack".to_string() } else { format!("Remove all {count}") };
            actions.push(button(ui::label(label)).style(ui::danger).on_press(Message::RemovePresetStacks(preset.id.clone())).into());
        }
        actions.push(dots(Message::ShowPresetMenu(preset.id.clone()), t));
        let detail = match count {
            0 => format!("{} · by {}", if preset.widget.len() == 1 { "1 widget".to_string() } else { format!("{} widgets", preset.widget.len()) }, preset.author),
            1 => "On your desktop".to_string(),
            n => format!("{n} on your desktop"),
        };
        card(self.previews.stack(&preset.id), preset.name.clone(), detail, actions, None, count > 0, t)
    }

    /// A placed stack's card on My widgets: click it for its page.
    pub(super) fn stack_card<'a>(&'a self, stack: &'a Stack, t: theme::Tokens) -> Element<'a, Message> {
        let members = self.widgets.members(stack.id).count();
        let detail = format!("Stack · {members} widgets · {}", super::where_on_screen(stack.x, stack.y));
        let actions: Vec<Element<'a, Message>> = vec![
            button(ui::label("Remove")).style(ui::secondary).on_press(Message::RemoveStack(stack.id)).into(),
            dots(Message::ShowStackMenu(stack.id), t),
        ];
        card(self.previews.stack(&stack.preset), stack.name.clone(), detail, actions, Some(Message::OpenStack(stack.id)), false, t)
    }

    /// A stack's own page.
    pub(super) fn stack_details<'a>(&'a self, stack: &'a Stack, t: theme::Tokens) -> Element<'a, Message> {
        let id = stack.id;
        let heading = row![
            button(ui::label("← My widgets")).style(ui::ghost).on_press(Message::Back),
            text(stack.name.as_str()).size(20).color(t.foreground),
            iced::widget::Space::new().width(Length::Fill),
            button(ui::label("Ungroup")).style(ui::secondary).on_press(Message::Ungroup(id)),
            button(ui::label("Remove stack")).style(ui::danger).on_press(Message::RemoveStack(id)),
        ]
        .spacing(12)
        .align_y(Alignment::Center);

        let first = self.widgets.members(id).next().map(|placed| placed.id);
        let screen = section(
            "Where it sits",
            "Your screen, with the stack outlined. Drag any part of it and the whole stack moves.",
            super::super::widgets_layout::view(&self.widgets.placed, first, self.screen, &self.previews, 380.0, t),
            t,
        );

        let size = section(
            "Size",
            "Makes every widget in the stack bigger or smaller together.",
            row![
                text("Size").size(13).color(t.foreground).width(Length::Fixed(110.0)),
                slider(0.5..=2.0, stack.scale, move |value| Message::StackScale(id, value)).step(0.05_f32).on_release(Message::Commit),
                text(format!("{:.0}%", stack.scale * 100.0)).size(13).color(t.muted_foreground).width(Length::Fixed(48.0)),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .into(),
            t,
        );

        let members: Vec<Element<'a, Message>> = self.widgets.members(id).map(|placed| self.placed_card(placed, t)).collect();
        let inside = section("In this stack", "Click a widget to change its look. Remove takes only that one out.", row(members).spacing(16).wrap().vertical_spacing(16).into(), t);

        column![heading, screen, size, inside].spacing(16).into()
    }
}

/// The ⋯ button every card has.
fn dots<'a>(message: Message, t: theme::Tokens) -> Element<'a, Message> {
    button(iced::widget::svg(crate::icons::ellipsis()).width(16).height(16).style(move |_theme, _status| iced::widget::svg::Style { color: Some(t.foreground) })).padding([6, 10]).style(ui::ghost).on_press(message).into()
}

impl State {
    /// The ⋯ menus of a stack and of a stack preset, over the whole window.
    pub(super) fn stack_overlay<'a>(&'a self, t: theme::Tokens) -> Option<Element<'a, Message>> {
        if let Some(stack) = self.menu_stack.and_then(|id| self.widgets.stack(id)) {
            let id = stack.id;
            let members: Vec<Element<'a, Message>> = self.widgets.members(id).map(|placed| self.placed_card(placed, t)).collect();
            let body = column![
                text("Size").size(16).color(t.foreground),
                row![
                    slider(0.5..=2.0, stack.scale, move |value| Message::StackScale(id, value)).step(0.05_f32).on_release(Message::Commit),
                    text(format!("{:.0}%", stack.scale * 100.0)).size(13).color(t.muted_foreground).width(Length::Fixed(48.0)),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
                text("In this stack").size(16).color(t.foreground),
                text("Remove takes only that widget out; ⋯ changes its look.").size(13).color(t.muted_foreground),
                row(members).spacing(16).wrap().vertical_spacing(16),
                Space::new().height(Length::Fixed(8.0)),
                row![
                    ui::tile("Open its page", "Where it sits, its size and everything in it", Message::OpenStack(id), false, t),
                    ui::tile("Ungroup", "Keep the widgets, each one on its own", Message::Ungroup(id), false, t),
                    ui::tile("Remove stack", "Takes the whole stack off; you can undo it", Message::RemoveStack(id), true, t),
                ]
                .spacing(12),
            ]
            .spacing(12);
            let subtitle = format!("Stack · {} · {}", count_label(self.widgets.members(id).count()), super::where_on_screen(stack.x, stack.y));
            return Some(ui::screen(stack.name.clone(), Some(subtitle), ui::scroll_page(body).into(), Message::CloseMenu, t));
        }
        let preset = self.menu_preset.as_deref().and_then(paperloom_common::stacks::preset)?;
        let copies: Vec<&Stack> = self.widgets.stacks.iter().filter(|stack| stack.preset == preset.id).collect();
        let mut body = column![ui::tile("Add to the desktop", if copies.is_empty() { "Puts the whole stack on your desktop" } else { "Another copy, next to the others" }, Message::AddStack(preset.id.clone()), false, t)].spacing(12);
        if !copies.is_empty() {
            body = body.push(text("On your desktop").size(16).color(t.foreground));
            for (index, stack) in copies.iter().enumerate() {
                body = body.push(
                    row![
                        text(format!("Copy {} · {}", index + 1, super::where_on_screen(stack.x, stack.y))).size(14).color(t.foreground).width(Length::Fill),
                        button(ui::label("Open")).style(ui::ghost).on_press(Message::OpenStack(stack.id)),
                        button(ui::label("Remove")).style(ui::secondary).on_press(Message::RemoveStack(stack.id)),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                );
            }
        }
        if preset.installed {
            body = body.push(ui::tile("Delete from this computer", "Removes the downloaded stack (copies on your desktop stay)", Message::RemovePreset(preset.folder.clone()), true, t));
        }
        let subtitle = format!("{} · by {}", count_label(preset.widget.len()), preset.author);
        Some(ui::screen(preset.name.clone(), Some(subtitle), ui::scroll_page(body).into(), Message::CloseMenu, t))
    }

    /// Everything on the desktop, as a list: stacks with the widgets inside them, then single widgets.
    pub(super) fn desktop_list<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let selected = self.selected;
        let line = |chosen: bool, content: Element<'a, Message>| -> Element<'a, Message> {
            container(content)
                .padding([6, 10])
                .width(Length::Fill)
                .style(move |_theme| container::Style {
                    background: chosen.then_some(Background::Color(t.muted)),
                    border: Border { radius: 6.0.into(), ..Border::default() },
                    ..container::Style::default()
                })
                .into()
        };
        let mut list = column![
            row![
                column![text("On your desktop").size(16).color(t.foreground), text("Click a name to find it on the screen above. Remove takes off only that one.").size(13).color(t.muted_foreground)].spacing(2).width(Length::Fill),
                button(ui::label("Remove everything")).style(ui::danger).on_press(Message::RemoveAll),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        ]
        .spacing(4);

        for stack in &self.widgets.stacks {
            let first = self.widgets.members(stack.id).next().map(|placed| placed.id);
            let chosen = selected.is_some_and(|id| self.widgets.members(stack.id).any(|placed| placed.id == id));
            list = list.push(line(
                chosen,
                row![
                    button(text(stack.name.clone()).size(14).color(t.foreground)).style(ui::ghost).padding([2, 4]).on_press_maybe(first.map(Message::Select)),
                    text(format!("stack · {}", count_label(self.widgets.members(stack.id).count()))).size(12).color(t.muted_foreground).width(Length::Fill),
                    button(ui::label("Open")).style(ui::ghost).on_press(Message::OpenStack(stack.id)),
                    button(ui::label("Ungroup")).style(ui::ghost).on_press(Message::Ungroup(stack.id)),
                    button(ui::label("Remove stack")).style(ui::danger).on_press(Message::RemoveStack(stack.id)),
                    dots(Message::ShowStackMenu(stack.id), t),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into(),
            ));
            for placed in self.widgets.members(stack.id) {
                list = list.push(line(selected == Some(placed.id), self.desktop_row(placed, 28, t)));
            }
        }
        for placed in self.widgets.placed.iter().filter(|placed| placed.stack == 0) {
            list = list.push(line(selected == Some(placed.id), self.desktop_row(placed, 0, t)));
        }
        container(list).padding(14).width(Length::Fill).style(move |_theme| container::Style { background: Some(Background::Color(t.surface)), border: Border { color: t.border, width: 1.0, radius: 8.0.into() }, ..container::Style::default() }).into()
    }

    /// One widget in the desktop list.
    fn desktop_row<'a>(&'a self, placed: &'a paperloom_common::widgets::Placed, indent: u16, t: theme::Tokens) -> Element<'a, Message> {
        let look = super::widgets_looks::skin_of(placed).map_or("", |skin| skin.name.as_str());
        row![
            Space::new().width(Length::Fixed(f32::from(indent))),
            button(text(self.title_of(placed)).size(14).color(t.foreground)).style(ui::ghost).padding([2, 4]).on_press(Message::Select(placed.id)),
            text(format!("{look} look · {}", super::where_on_screen(placed.x, placed.y))).size(12).color(t.muted_foreground).width(Length::Fill),
            button(ui::label("Open")).style(ui::ghost).on_press(Message::Open(placed.id)),
            button(ui::label("Remove")).style(ui::secondary).on_press(Message::Remove(placed.id)),
            dots(Message::ShowMenu(placed.id), t),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    }
}

fn count_label(count: usize) -> String {
    if count == 1 { "1 widget".to_string() } else { format!("{count} widgets") }
}
