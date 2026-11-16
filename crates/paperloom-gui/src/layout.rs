//! Where the theme puts things: how the navigation shows for a given
//! window width, and what is drawn behind and in front of the pages.

use iced::widget::{container, image, stack, Space};
use iced::{Alignment, Background, ContentFit, Element, Length};
use paperloom_theme::{Anchor, Layout, NavStyle};

use crate::app::Page;
use crate::theme::{self, Active, LayerPictures};
use crate::ui::Width;

/// How the navigation shows right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    /// The whole tree at the side.
    Tree,
    /// Icons at the side.
    Rail,
    /// A bar across the top or bottom, with or without labels.
    Bar { labels: bool },
    /// Nothing but a menu button in the top bar.
    MenuOnly,
    /// Narrow window: folded into the top bar (wins over any theme).
    Folded,
}

impl Nav {
    pub fn of(layout: Layout, width: Width) -> Self {
        if width == Width::Narrow {
            return Nav::Folded;
        }
        let at_side = matches!(layout.nav, paperloom_theme::Nav::Left | paperloom_theme::Nav::Right);
        match (layout.nav_style, at_side) {
            (NavStyle::Hidden, _) => Nav::MenuOnly,
            (NavStyle::Full, true) if width == Width::Wide => Nav::Tree,
            (_, true) => Nav::Rail,
            (NavStyle::Full, false) => Nav::Bar { labels: width == Width::Wide },
            (_, false) => Nav::Bar { labels: false },
        }
    }

    /// Width the side navigation takes from the page.
    pub fn side_width(self, layout: Layout) -> f32 {
        match self {
            Nav::Tree => layout.nav_width,
            Nav::Rail => 64.0,
            _ => 0.0,
        }
    }

    /// A side navigation: `Some(true)` the tree, `Some(false)` the icon rail.
    pub fn side(self) -> Option<bool> {
        match self {
            Nav::Tree => Some(true),
            Nav::Rail => Some(false),
            _ => None,
        }
    }

    /// A bar across the window: `Some(labels)`.
    pub fn bar(self) -> Option<bool> {
        match self {
            Nav::Bar { labels } => Some(labels),
            _ => None,
        }
    }
}

/// The name a theme's `pages = [...]` uses for the page being shown.
pub fn page_group(page: Page) -> &'static str {
    match page {
        Page::Library => "library",
        Page::Online | Page::Downloads | Page::Sources => "online",
        Page::Widgets | Page::WidgetsBrowse | Page::WidgetsAdd | Page::WidgetsLayout | Page::WidgetsStacks => "widgets",
        _ => "settings",
    }
}

fn shows_on(layer: &LayerPictures, page: &str) -> bool {
    layer.spec.pages.is_empty() || layer.spec.pages.iter().any(|wanted| wanted.eq_ignore_ascii_case(page))
}

/// One picture layer, placed by its anchor. It fills the window but is only
/// a picture, so the mouse goes straight through to what is under it.
fn layer<'a, Message: 'a>(layer: &LayerPictures, frame: usize) -> Element<'a, Message> {
    let spec = &layer.spec;
    let handle = layer.frames[frame % layer.frames.len()].clone();
    let mut picture = image(handle).opacity(spec.opacity.clamp(0.0, 1.0));
    if spec.width > 0.0 {
        picture = picture.width(Length::Fixed(spec.width));
    }
    if spec.height > 0.0 {
        picture = picture.height(Length::Fixed(spec.height));
    }
    let (x, y) = match spec.anchor {
        Anchor::TopLeft => (Alignment::Start, Alignment::Start),
        Anchor::Top => (Alignment::Center, Alignment::Start),
        Anchor::TopRight => (Alignment::End, Alignment::Start),
        Anchor::Left => (Alignment::Start, Alignment::Center),
        Anchor::Center => (Alignment::Center, Alignment::Center),
        Anchor::Right => (Alignment::End, Alignment::Center),
        Anchor::BottomLeft => (Alignment::Start, Alignment::End),
        Anchor::Bottom => (Alignment::Center, Alignment::End),
        Anchor::BottomRight => (Alignment::End, Alignment::End),
    };
    container(picture).width(Length::Fill).height(Length::Fill).align_x(x).align_y(y).padding(spec.margin.max(0.0)).into()
}

/// `content` on the theme's backdrop: the background colour or gradient,
/// then its picture, then the picture layers that go behind the pages.
pub fn backdrop<'a, Message: 'a>(active: &Active, t: theme::Tokens, page: &str, frame: usize, content: Element<'a, Message>) -> Element<'a, Message> {
    let background = if active.gradient.is_empty() {
        Background::Color(t.background)
    } else {
        let angle = active.spec().backdrop.angle.to_radians() + std::f32::consts::FRAC_PI_2;
        let last = (active.gradient.len().min(8) - 1).max(1) as f32;
        let gradient = active.gradient.iter().take(8).enumerate().fold(iced::gradient::Linear::new(iced::Radians(angle)), |gradient, (index, color)| gradient.add_stop(index as f32 / last, *color));
        Background::Gradient(iced::Gradient::Linear(gradient))
    };
    let base = move |inside: Element<'a, Message>| -> Element<'a, Message> {
        container(inside).width(Length::Fill).height(Length::Fill).style(move |_theme| container::Style { background: Some(background), ..container::Style::default() }).into()
    };

    let behind: Vec<&LayerPictures> = active.layers.iter().filter(|layer| !layer.spec.front && shows_on(layer, page)).collect();
    if active.backdrop.is_none() && behind.is_empty() {
        return base(content);
    }
    let mut layers = stack![base(Space::new().into())].width(Length::Fill).height(Length::Fill);
    if let Some(handle) = &active.backdrop {
        let spec = &active.spec().backdrop;
        let fit = match spec.fit.as_str() {
            "contain" => ContentFit::Contain,
            "stretch" => ContentFit::Fill,
            _ => ContentFit::Cover,
        };
        layers = layers.push(image(handle.clone()).width(Length::Fill).height(Length::Fill).content_fit(fit).opacity(spec.opacity.clamp(0.0, 1.0)));
    }
    for picture in behind {
        layers = layers.push(layer(picture, frame));
    }
    layers.push(content).into()
}

/// The theme's pictures that go in front of everything.
pub fn front_layers<'a, Message: 'a>(active: &Active, page: &str, frame: usize, content: Element<'a, Message>) -> Element<'a, Message> {
    let front: Vec<&LayerPictures> = active.layers.iter().filter(|layer| layer.spec.front && shows_on(layer, page)).collect();
    if front.is_empty() {
        return content;
    }
    let mut layers = stack![content].width(Length::Fill).height(Length::Fill);
    for picture in front {
        layers = layers.push(layer(picture, frame));
    }
    layers.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperloom_theme::Nav as Side;

    #[test]
    fn a_narrow_window_always_folds() {
        for nav in Side::ALL {
            for nav_style in NavStyle::ALL {
                assert_eq!(Nav::of(Layout { nav, nav_style, ..Layout::default() }, Width::Narrow), Nav::Folded);
            }
        }
    }

    #[test]
    fn the_default_layout_is_the_one_the_app_always_had() {
        let layout = Layout::default();
        assert_eq!(Nav::of(layout, Width::Wide), Nav::Tree);
        assert_eq!(Nav::of(layout, Width::Wide).side_width(layout), 200.0);
        assert_eq!(Nav::of(layout, Width::Medium), Nav::Rail);
        assert_eq!(Nav::of(layout, Width::Medium).side_width(layout), 64.0);
    }

    #[test]
    fn themes_move_the_navigation() {
        let top = Layout { nav: Side::Top, ..Layout::default() };
        assert_eq!(Nav::of(top, Width::Wide), Nav::Bar { labels: true });
        assert_eq!(Nav::of(top, Width::Medium), Nav::Bar { labels: false });
        assert_eq!(Nav::of(top, Width::Wide).side_width(top), 0.0);
        let hidden = Layout { nav_style: NavStyle::Hidden, ..Layout::default() };
        assert_eq!(Nav::of(hidden, Width::Wide), Nav::MenuOnly);
        let icons = Layout { nav: Side::Right, nav_style: NavStyle::Icons, ..Layout::default() };
        assert_eq!(Nav::of(icons, Width::Wide), Nav::Rail);
    }
}
