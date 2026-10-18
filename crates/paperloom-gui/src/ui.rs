//! Small layout helpers shared by every page, so the UI holds up at any
//! window size — from a phone-narrow strip to a very short, very wide one.

use iced::widget::{container, scrollable, text, Text};
use iced::{Element, Padding};

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
