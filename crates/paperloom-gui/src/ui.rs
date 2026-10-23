//! Small layout helpers shared by every page, so the UI holds up at any
//! window size — from a phone-narrow strip to a very short, very wide one.

use iced::widget::{button, container, row, scrollable, text, Row, Text};
use iced::{Background, Border, Color, Element, Padding, Shadow};

use crate::theme;

/// The colours the current window is drawn with, so button styles (plain
/// functions iced calls with only the palette) can use the app's own tokens
/// instead of iced's generated greys. Set at the top of every `view`.
static TOKENS: std::sync::RwLock<Option<theme::Tokens>> = std::sync::RwLock::new(None);

pub fn set_tokens(t: theme::Tokens) {
    *TOKENS.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(t);
}

fn tokens() -> theme::Tokens {
    TOKENS.read().unwrap_or_else(|poisoned| poisoned.into_inner()).unwrap_or_else(|| theme::tokens(paperloom_common::ThemeMode::Dark))
}

fn mix(a: Color, b: Color, amount: f32) -> Color {
    Color::from_rgb(a.r + (b.r - a.r) * amount, a.g + (b.g - a.g) * amount, a.b + (b.b - a.b) * amount)
}

fn luminance(c: Color) -> f32 {
    0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
}

fn fade(style: button::Style, status: button::Status) -> button::Style {
    if matches!(status, button::Status::Disabled) {
        button::Style {
            background: style.background.map(|background| match background {
                Background::Color(color) => Background::Color(Color { a: color.a * 0.45, ..color }),
                other => other,
            }),
            text_color: Color { a: style.text_color.a * 0.5, ..style.text_color },
            border: Border { color: Color { a: style.border.color.a * 0.5, ..style.border.color }, ..style.border },
            ..style
        }
    } else {
        style
    }
}

fn base(background: Option<Color>, text_color: Color, border: Option<(Color, f32)>) -> button::Style {
    button::Style {
        background: background.map(Background::Color),
        text_color,
        border: Border { color: border.map_or(Color::TRANSPARENT, |(c, _)| c), width: border.map_or(0.0, |(_, w)| w), radius: 8.0.into() },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// The main action: the accent colour.
pub fn primary(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let t = tokens();
    let on_accent = if luminance(t.primary) > 0.55 { Color::from_rgb(0.08, 0.06, 0.05) } else { Color::WHITE };
    let background = match status {
        button::Status::Hovered => t.primary_hover,
        button::Status::Pressed => mix(t.primary, Color::BLACK, 0.18),
        _ => t.primary,
    };
    fade(base(Some(background), on_accent, None), status)
}

/// A quiet button: raised panel with a hairline border.
pub fn secondary(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let t = tokens();
    let background = match status {
        button::Status::Hovered => mix(t.muted, t.foreground, 0.10),
        button::Status::Pressed => mix(t.muted, t.foreground, 0.16),
        _ => t.muted,
    };
    let border = if matches!(status, button::Status::Hovered | button::Status::Pressed) { Color { a: 0.5, ..t.primary } } else { t.border };
    fade(base(Some(background), t.foreground, Some((border, 1.0))), status)
}

/// Removing or deleting: a tinted red, not a loud one.
pub fn danger(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let t = tokens();
    let strength = match status {
        button::Status::Hovered => 0.30,
        button::Status::Pressed => 0.40,
        _ => 0.16,
    };
    fade(base(Some(Color { a: strength, ..t.destructive }), t.destructive, Some((Color { a: 0.45, ..t.destructive }, 1.0))), status)
}

/// A text-only button (no box until hovered).
pub fn ghost(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let t = tokens();
    let (background, text_color) = match status {
        button::Status::Hovered => (Some(t.muted), t.foreground),
        button::Status::Pressed => (Some(mix(t.muted, t.foreground, 0.10)), t.foreground),
        _ => (None, t.muted_foreground),
    };
    fade(base(background, text_color, None), status)
}

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
