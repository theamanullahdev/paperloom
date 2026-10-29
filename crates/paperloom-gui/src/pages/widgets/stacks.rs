//! Stacks on the Widgets pages: the Stacks page (ready-made sets to put on the
//! desktop, installed or from the online library), a stack's card on My
//! widgets, and a stack's own page (where it sits, its size, what is in it).
//! They use the same cards as everything else here.

use iced::widget::{button, column, row, slider, text};
use iced::{Alignment, Element, Length};

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
        let mut actions: Vec<Element<'a, Message>> = vec![button(ui::label("Add to desktop").color(ui::on_accent())).style(ui::primary).on_press(Message::AddStack(preset.id.clone())).into()];
        if preset.installed {
            actions.push(button(ui::label("Remove")).style(ui::ghost).on_press(Message::RemovePreset(preset.folder.clone())).into());
        }
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
            button(ui::label("Ungroup")).style(ui::ghost).on_press(Message::Ungroup(stack.id)).into(),
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
