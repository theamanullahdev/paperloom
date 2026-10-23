//! Library → Rotation: change the wallpaper by itself. One always-on rotation
//! (a collection, how often, shuffle) and a weekly schedule of rules ("on
//! weekday evenings use this collection"). Saved to `rotation.toml`, which
//! the engine reads and acts on; this page only edits it.

use iced::widget::{button, column, container, pick_list, row, text, toggler, Space};
use iced::{Alignment, Background, Border, Element, Length};

use paperloom_common::ipc::{self, Command};
use paperloom_common::rotation::{Rotation, Rule};
use paperloom_common::Collection;

use crate::theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    id: u64,
    name: String,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Every(u32);

const INTERVALS: [Every; 9] = [Every(0), Every(5), Every(10), Every(15), Every(30), Every(60), Every(120), Every(360), Every(1440)];

impl std::fmt::Display for Every {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            0 => f.write_str("Never (stay on one)"),
            m if m < 60 => write!(f, "Every {m} minutes"),
            60 => f.write_str("Every hour"),
            m if m < 1440 => write!(f, "Every {} hours", m / 60),
            _ => f.write_str("Every day"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock(u16);

impl std::fmt::Display for Clock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:02}:{:02}", self.0 / 60, self.0 % 60)
    }
}

fn clocks() -> Vec<Clock> {
    (0..48).map(|step| Clock(step * 30)).collect()
}

#[derive(Debug, Clone)]
pub enum Message {
    Enabled(bool),
    Collection(Choice),
    Interval(Every),
    Shuffle(bool),
    Next,
    AddRule,
    RemoveRule(usize),
    RuleEnabled(usize, bool),
    RuleCollection(usize, Choice),
    RuleStart(usize, Clock),
    RuleEnd(usize, Clock),
    RuleDay(usize, u8),
}

pub struct State {
    rotation: Rotation,
    note: Option<String>,
}

impl State {
    pub fn new() -> Self {
        Self { rotation: Rotation::load(), note: None }
    }

    pub fn update(&mut self, message: Message, collections: &[Collection]) {
        let rotation = &mut self.rotation;
        match message {
            Message::Enabled(on) => {
                rotation.enabled = on;
                if on && rotation.collection.is_none() {
                    rotation.collection = collections.first().map(|collection| collection.id);
                }
            }
            Message::Collection(choice) => rotation.collection = Some(choice.id),
            Message::Interval(Every(minutes)) => rotation.interval_minutes = minutes,
            Message::Shuffle(on) => rotation.shuffle = on,
            Message::Next => {
                self.note = Some(match ipc::send(&Command::Next) {
                    Ok(()) => "Changing to the next wallpaper…".into(),
                    Err(_) => "Start a wallpaper first — rotation runs in the wallpaper engine.".into(),
                });
                return;
            }
            Message::AddRule => rotation.rules.push(Rule { collection: collections.first().map_or(0, |collection| collection.id), ..Rule::default() }),
            Message::RemoveRule(index) => {
                if index < rotation.rules.len() {
                    rotation.rules.remove(index);
                }
            }
            Message::RuleEnabled(index, on) => with_rule(rotation, index, |rule| rule.enabled = on),
            Message::RuleCollection(index, choice) => with_rule(rotation, index, |rule| rule.collection = choice.id),
            Message::RuleStart(index, Clock(minutes)) => with_rule(rotation, index, |rule| rule.start = minutes),
            Message::RuleEnd(index, Clock(minutes)) => with_rule(rotation, index, |rule| rule.end = minutes),
            Message::RuleDay(index, day) => with_rule(rotation, index, |rule| rule.days ^= 1 << day),
        }
        self.note = None;
        let _ = self.rotation.save();
    }

    pub fn view<'a>(&'a self, collections: &'a [Collection], t: theme::Tokens) -> Element<'a, Message> {
        if collections.is_empty() {
            return container(text("Make a collection first (Library → Collections). Rotation and schedules play the wallpapers in a collection.").color(t.muted_foreground)).padding(24).into();
        }
        let choices: Vec<Choice> = collections.iter().map(|collection| Choice { id: collection.id, name: collection.name.clone() }).collect();
        let picked = |id: Option<u64>| id.and_then(|id| choices.iter().find(|choice| choice.id == id).cloned());
        let rotation = &self.rotation;

        let always = card(
            column![
                heading("Rotate wallpapers", t),
                toggler(rotation.enabled).label("Change the wallpaper automatically").on_toggle(Message::Enabled),
                row![
                    text("Collection").size(13).width(Length::Fixed(90.0)),
                    pick_list(choices.clone(), picked(rotation.collection), Message::Collection).placeholder("Choose a collection"),
                ]
                .align_y(Alignment::Center)
                .spacing(8),
                row![text("How often").size(13).width(Length::Fixed(90.0)), pick_list(INTERVALS, Some(Every(rotation.interval_minutes)), Message::Interval)].align_y(Alignment::Center).spacing(8),
                toggler(rotation.shuffle).label("Shuffle").on_toggle(Message::Shuffle),
                row![
                    button(crate::ui::label("Next wallpaper now")).style(crate::ui::secondary).on_press(Message::Next),
                    text(self.note.clone().unwrap_or_default()).size(12).color(t.muted_foreground),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
                text("Runs in the wallpaper engine, so it keeps going when the Paperloom window is closed.").size(12).color(t.muted_foreground),
            ]
            .spacing(10)
            .into(),
            t,
        );

        let mut schedule = column![
            heading("Schedule", t),
            text("A rule beats the rotation above while it applies — for example an evening collection on weekdays.").size(12).color(t.muted_foreground),
        ]
        .spacing(10);
        if rotation.rules.is_empty() {
            schedule = schedule.push(text("No rules yet.").size(13).color(t.muted_foreground));
        }
        for (index, rule) in rotation.rules.iter().enumerate() {
            schedule = schedule.push(rule_row(index, rule, &choices, picked(Some(rule.collection)), t));
        }
        schedule = schedule.push(button(crate::ui::label("Add a rule")).style(crate::ui::secondary).on_press(Message::AddRule));

        column![always, card(schedule.into(), t)].spacing(14).into()
    }
}

fn with_rule(rotation: &mut Rotation, index: usize, change: impl FnOnce(&mut Rule)) {
    if let Some(rule) = rotation.rules.get_mut(index) {
        change(rule);
    }
}

fn rule_row<'a>(index: usize, rule: &'a Rule, choices: &[Choice], picked: Option<Choice>, t: theme::Tokens) -> Element<'a, Message> {
    let days: Vec<Element<'a, Message>> = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
        .iter()
        .enumerate()
        .map(|(day, name)| {
            let on = rule.days & (1 << day) != 0;
            button(crate::ui::label(*name).size(12)).style(if on { crate::ui::primary } else { crate::ui::secondary }).padding([3, 8]).on_press(Message::RuleDay(index, day as u8)).into()
        })
        .collect();
    container(
        column![
            row![
                toggler(rule.enabled).on_toggle(move |on| Message::RuleEnabled(index, on)),
                pick_list(choices.to_vec(), picked, move |choice| Message::RuleCollection(index, choice)).placeholder("Collection"),
                text("from").size(13),
                pick_list(clocks(), Some(Clock(rule.start)), move |clock| Message::RuleStart(index, clock)),
                text("to").size(13),
                pick_list(clocks(), Some(Clock(rule.end)), move |clock| Message::RuleEnd(index, clock)),
                Space::new().width(Length::Fill),
                button(crate::ui::label("Remove").size(12)).style(crate::ui::ghost).on_press(Message::RemoveRule(index)),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .wrap()
            .vertical_spacing(6),
            row(days).spacing(4),
        ]
        .spacing(8),
    )
    .padding(10)
    .width(Length::Fill)
    .style(move |_theme| container::Style { border: Border { color: t.border, width: 1.0, radius: 8.0.into() }, ..container::Style::default() })
    .into()
}

fn heading<'a>(label: &str, t: theme::Tokens) -> Element<'a, Message> {
    text(label.to_string()).font(theme::BODY_FONT).size(15).color(t.foreground).into()
}

fn card<'a>(content: Element<'a, Message>, t: theme::Tokens) -> Element<'a, Message> {
    container(content)
        .padding(14)
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(t.surface)),
            border: Border { color: t.border, width: 1.0, radius: 10.0.into() },
            ..container::Style::default()
        })
        .into()
}
