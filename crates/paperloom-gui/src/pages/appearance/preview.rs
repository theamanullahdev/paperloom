//! A small drawing of the window in a theme's own colours, shape and
//! layout: the thumbnail of a theme that is on this computer. Drawn by the
//! app from the theme file, so it is always true to the theme.

use iced::widget::{column, container, row, Space};
use iced::{Background, Border, Color, Element, Length};
use paperloom_theme::{Nav, NavStyle, Theme};

fn color(value: paperloom_theme::Rgba) -> Color {
    Color::from_rgba(value[0], value[1], value[2], value[3])
}

fn block<'a, Message: 'a>(fill: Color, radius: f32) -> container::Container<'a, Message> {
    container(Space::new()).style(move |_theme| container::Style { background: Some(Background::Color(fill)), border: Border { radius: radius.into(), ..Border::default() }, ..container::Style::default() })
}

/// The window in miniature, `width` pixels wide (its height follows the
/// usual window shape).
pub fn mock<'a, Message: 'a>(theme: &Theme, dark: bool, width: f32) -> Element<'a, Message> {
    let palette = theme.palette(dark);
    let shape = theme.shape.clamped();
    let scale = width / 320.0;
    let height = width * 0.62;
    let radius = move |pixels: f32| (pixels * scale * 1.6).min(10.0 * scale);
    let (background, surface, border, foreground, muted_foreground, primary) = (color(palette.background), color(palette.surface), color(palette.border), color(palette.foreground), color(palette.muted_foreground), color(palette.primary));
    let part_color = |name: &str, fallback: Color| theme.part.get(name).and_then(|part| part.background.as_deref()).and_then(paperloom_theme::parse_color).map(color).unwrap_or(fallback);
    let nav_fill = part_color("nav", surface);
    let top_fill = part_color("top_bar", surface);

    let line = move |fill: Color, w: f32| block(fill, 2.0 * scale).width(Length::Fixed(w * scale)).height(Length::Fixed(5.0 * scale));
    let card = move || {
        container(column![line(foreground, 54.0), line(muted_foreground, 38.0)].spacing(4.0 * scale))
            .padding(8.0 * scale)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme| container::Style { background: Some(Background::Color(surface)), border: Border { color: border, width: shape.border_width.min(2.0), radius: radius(shape.card_radius).into() }, ..container::Style::default() })
    };
    let button = block(part_color("button_primary", primary), radius(shape.control_radius)).width(Length::Fixed(46.0 * scale)).height(Length::Fixed(13.0 * scale));

    let top_bar = container(row![line(foreground, 40.0), Space::new().width(Length::Fill), button].align_y(iced::Alignment::Center))
        .padding([6.0 * scale, 9.0 * scale])
        .width(Length::Fill)
        .style(move |_theme| container::Style { background: Some(Background::Color(top_fill)), ..container::Style::default() });
    let page = column![top_bar, container(row![card(), card(), card()].spacing(7.0 * scale)).padding(9.0 * scale).height(Length::Fill)].width(Length::Fill).height(Length::Fill);

    let at_side = matches!(theme.layout.nav, Nav::Left | Nav::Right);
    let nav_items = move |count: usize, w: f32| -> Vec<Element<'a, Message>> {
        (0..count).map(|index| line(if index == 0 { primary } else { muted_foreground }, w).into()).collect()
    };
    let nav: Option<Element<'a, Message>> = match (theme.layout.nav_style, at_side) {
        (NavStyle::Hidden, _) => None,
        (style, true) => {
            let wide = style == NavStyle::Full;
            Some(
                container(column(nav_items(5, if wide { 44.0 } else { 10.0 })).spacing(8.0 * scale))
                    .padding(9.0 * scale)
                    .width(Length::Fixed((if wide { 70.0 } else { 30.0 }) * scale))
                    .height(Length::Fill)
                    .style(move |_theme| container::Style { background: Some(Background::Color(nav_fill)), ..container::Style::default() })
                    .into(),
            )
        }
        (_, false) => Some(
            container(row(nav_items(4, 30.0)).spacing(10.0 * scale))
                .padding([7.0 * scale, 9.0 * scale])
                .width(Length::Fill)
                .style(move |_theme| container::Style { background: Some(Background::Color(nav_fill)), ..container::Style::default() })
                .into(),
        ),
    };
    let window: Element<'a, Message> = match (nav, theme.layout.nav) {
        (None, _) => page.into(),
        (Some(nav), Nav::Left) => row![nav, page].into(),
        (Some(nav), Nav::Right) => row![page, nav].into(),
        (Some(nav), Nav::Top) => column![nav, page].into(),
        (Some(nav), Nav::Bottom) => column![page, nav].into(),
    };

    // The backdrop's gradient shows as the window's background.
    let gradient: Vec<Color> = theme.backdrop.gradient.iter().filter_map(|written| paperloom_theme::parse_color(written)).map(color).collect();
    let fill = if gradient.len() >= 2 {
        let last = (gradient.len().min(8) - 1) as f32;
        let angle = theme.backdrop.angle.to_radians() + std::f32::consts::FRAC_PI_2;
        Background::Gradient(iced::Gradient::Linear(gradient.iter().take(8).enumerate().fold(iced::gradient::Linear::new(iced::Radians(angle)), |built, (index, stop)| built.add_stop(index as f32 / last, *stop))))
    } else {
        Background::Color(background)
    };
    container(window)
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .clip(true)
        .style(move |_theme| container::Style { background: Some(fill), border: Border { color: border, width: 1.0, radius: (6.0 * scale.max(0.5)).into() }, ..container::Style::default() })
        .into()
}
