//! Small layout helpers shared by every page, so the UI holds up at any
//! window size — from a phone-narrow strip to a very short, very wide one.

use iced::widget::{button, container, row, scrollable, text, Row, Text};
use iced::{Background, Border, Color, Element, Padding, Shadow};

use crate::theme;

/// Text for a button or chip: never wrapped mid-label. A crowded row
/// should move whole buttons to the next line (`row(..).wrap()`), not break
/// "Stop and quit Paperloom" into one word per line.
pub fn label<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(content).wrapping(text::Wrapping::None)
}

/// Window widths at which the layout changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    /// Full sidebar with labels.
    Wide,
    /// Icon-only sidebar.
    Medium,
    /// No sidebar; navigation moves into the top bar.
    Narrow,
}

impl Width {
    pub fn of(width: f32) -> Self {
        if width >= 900.0 {
            Width::Wide
        } else if width >= 520.0 {
            Width::Medium
        } else {
            Width::Narrow
        }
    }
}

/// A page's scrolling content, with room on the right for the scrollbar so
/// it never sits on top of cards and their borders.
pub fn scroll_page<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> scrollable::Scrollable<'a, Message> {
    scrollable(container(content).padding(Padding { top: 4.0, right: 16.0, bottom: 8.0, left: 4.0 }))
}

/// A row of joined toggle buttons: one choice out of a few (tabs, view
/// size, theme). `items`: label, selected, message.
pub fn segmented<'a, Message: Clone + 'a>(items: Vec<(String, bool, Message)>, t: theme::Tokens) -> Element<'a, Message> {
    let buttons = items.into_iter().map(|(text, active, message)| segment(label(text).size(13).into(), active, message, t));
    segment_bar(row(buttons).spacing(2), t)
}

/// One button of a `segmented` bar.
pub fn segment<'a, Message: Clone + 'a>(content: Element<'a, Message>, active: bool, message: Message, t: theme::Tokens) -> Element<'a, Message> {
    button(content)
        .padding([5, 12])
        .on_press(message)
        .style(move |_theme, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: if active {
                    Some(Background::Color(t.surface))
                } else if hovered {
                    Some(Background::Color(Color { a: 0.5, ..t.surface }))
                } else {
                    None
                },
                text_color: if active { t.primary } else { t.foreground },
                border: Border { color: if active { t.border } else { Color::TRANSPARENT }, width: 1.0, radius: 7.0.into() },
                shadow: Shadow::default(),
                snap: false,
            }
        })
        .into()
}

/// The sunken track behind a `segmented` bar.
pub fn segment_bar<'a, Message: 'a>(content: Row<'a, Message>, t: theme::Tokens) -> Element<'a, Message> {
    container(content)
        .padding(3)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(t.muted)),
            border: Border { radius: 9.0.into(), ..Border::default() },
            ..container::Style::default()
        })
        .into()
}
