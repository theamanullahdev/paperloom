//! Pickers for the theme editor: a colour picker and a font picker, so
//! nobody has to type a value (a hex field and a font name box are still
//! there for people who want them). Both are pop-ups the editor shows over
//! itself.

use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::widget::{button, column, container, row, scrollable, slider, text, text_input, Space};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Point, Rectangle, Renderer, Size, Theme};

use crate::{theme, ui};

/// Hue 0–360, saturation, value and opacity 0–1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hsva {
    pub h: f32,
    pub s: f32,
    pub v: f32,
    pub a: f32,
}

impl Hsva {
    pub fn from_rgba(c: [f32; 4]) -> Self {
        let (r, g, b) = (c[0], c[1], c[2]);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;
        let h = if delta == 0.0 {
            0.0
        } else if max == r {
            60.0 * (((g - b) / delta).rem_euclid(6.0))
        } else if max == g {
            60.0 * ((b - r) / delta + 2.0)
        } else {
            60.0 * ((r - g) / delta + 4.0)
        };
        Hsva { h, s: if max == 0.0 { 0.0 } else { delta / max }, v: max, a: c[3] }
    }

    pub fn to_rgba(self) -> [f32; 4] {
        let c = self.v * self.s;
        let x = c * (1.0 - ((self.h / 60.0).rem_euclid(2.0) - 1.0).abs());
        let m = self.v - c;
        let (r, g, b) = match (self.h.rem_euclid(360.0) / 60.0) as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        [r + m, g + m, b + m, self.a]
    }

    pub fn hex(self) -> String {
        paperloom_theme::to_hex(self.to_rgba())
    }

    fn color(self) -> Color {
        let c = self.to_rgba();
        Color::from_rgba(c[0], c[1], c[2], c[3])
    }
}

#[derive(Debug, Clone)]
pub enum Colour {
    /// The square: saturation (left to right) and brightness (bottom to top).
    Square(f32, f32),
    Hue(f32),
    Opacity(f32),
    Swatch([f32; 4]),
    Hex(String),
    /// Back to what it was when the picker opened.
    Undo,
    /// Back to the theme's own default for this colour.
    Default,
    Done,
}

/// The colour picker's state: what is being edited and what it was.
pub struct ColourPicker {
    pub title: String,
    pub hsva: Hsva,
    hex: String,
    /// The colour when the picker opened (for Undo): `None` = it was not set.
    pub before: Option<String>,
    /// Colours worth offering: the theme's own, first.
    swatches: Vec<[f32; 4]>,
}

const BASICS: [&str; 24] = [
    "#000000", "#1C1917", "#3F3F46", "#71717A", "#A1A1AA", "#E4E4E7", "#F5F5F4", "#FFFFFF", "#DC2626", "#EA580C", "#F59E0B", "#EAB308", "#84CC16", "#16A34A", "#14B8A6", "#06B6D4", "#0EA5E9", "#6366F1", "#8B5CF6", "#D946EF",
    "#EC4899", "#F43F5E", "#9A3412", "#FB923C",
];

impl ColourPicker {
    pub fn new(title: impl Into<String>, current: [f32; 4], before: Option<String>, theme_colours: &[[f32; 4]]) -> Self {
        let hsva = Hsva::from_rgba(current);
        let mut swatches: Vec<[f32; 4]> = Vec::new();
        for colour in theme_colours.iter().copied().chain(BASICS.iter().filter_map(|hex| paperloom_theme::parse_color(hex))) {
            if !swatches.iter().any(|known| known == &colour) {
                swatches.push(colour);
            }
        }
        Self { title: title.into(), hsva, hex: hsva.hex(), before, swatches }
    }

    /// Applies a change; `true` when the colour itself changed (the editor then writes it into the theme).
    pub fn update(&mut self, message: &Colour) -> bool {
        match message {
            Colour::Square(s, v) => {
                self.hsva.s = s.clamp(0.0, 1.0);
                self.hsva.v = v.clamp(0.0, 1.0);
            }
            Colour::Hue(h) => self.hsva.h = h.clamp(0.0, 359.9),
            Colour::Opacity(a) => self.hsva.a = a.clamp(0.0, 1.0),
            Colour::Swatch(colour) => self.hsva = Hsva::from_rgba(*colour),
            Colour::Hex(written) => {
                self.hex = written.clone();
                if let Some(colour) = paperloom_theme::parse_color(written) {
                    self.hsva = Hsva::from_rgba(colour);
                    return true;
                }
                return false;
            }
            Colour::Undo | Colour::Default | Colour::Done => return false,
        }
        self.hex = self.hsva.hex();
        true
    }

    pub fn view<'a, Message: Clone + 'a>(&'a self, wrap: impl Fn(Colour) -> Message + Copy + 'a, close: Message, t: theme::Tokens) -> Element<'a, Message> {
        let hsva = self.hsva;
        let square = canvas::Canvas::new(Square { hsva, on_change: move |s, v| wrap(Colour::Square(s, v)) }).width(Length::Fixed(226.0)).height(Length::Fixed(170.0));
        let hue = column![text("Hue").size(12).color(t.muted_foreground), slider(0.0..=359.0, hsva.h, move |h| wrap(Colour::Hue(h))).step(1.0_f32)].spacing(2);
        let opacity = column![
            text(format!("See-through: {:.0}% solid", hsva.a * 100.0)).size(12).color(t.muted_foreground),
            slider(0.0..=1.0, hsva.a, move |a| wrap(Colour::Opacity(a))).step(0.01_f32)
        ]
        .spacing(2);
        let current = container(Space::new()).width(Length::Fixed(58.0)).height(Length::Fixed(58.0)).style(move |_theme| container::Style {
            background: Some(Background::Color(hsva.color())),
            border: Border { color: t.border, width: 1.0, radius: theme::radius_control().into() },
            ..container::Style::default()
        });

        let mut grid = column![].spacing(4);
        for chunk in self.swatches.chunks(8) {
            let mut line = row![].spacing(4);
            for colour in chunk {
                let fill = Color::from_rgba(colour[0], colour[1], colour[2], colour[3]);
                let colour = *colour;
                line = line.push(
                    button(Space::new())
                        .width(Length::Fixed(26.0))
                        .height(Length::Fixed(26.0))
                        .padding(0)
                        .on_press(wrap(Colour::Swatch(colour)))
                        .style(move |_theme, status| button::Style {
                            background: Some(Background::Color(fill)),
                            border: Border { color: if matches!(status, button::Status::Hovered) { t.primary } else { t.border }, width: if matches!(status, button::Status::Hovered) { 2.0 } else { 1.0 }, radius: 4.0.into() },
                            ..button::Style::default()
                        }),
                );
            }
            grid = grid.push(line);
        }

        let valid = paperloom_theme::parse_color(&self.hex).is_some();
        let content = column![
            text(self.title.clone()).font(theme::display_font()).size(18).color(t.foreground),
            row![square, column![current, text("now").size(11).color(t.muted_foreground)].spacing(4).align_x(Alignment::Center)].spacing(12),
            hue,
            opacity,
            text("Colours to start from").size(12).color(t.muted_foreground),
            grid,
            row![
                text("Or type it").size(12).color(t.muted_foreground),
                text_input("#RRGGBB", &self.hex).on_input(move |written| wrap(Colour::Hex(written))).padding(6).size(13).font(Font::MONOSPACE).width(Length::Fixed(120.0)),
                text(if valid { "" } else { "not a colour yet" }).size(12).color(t.destructive),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            row![
                button(ui::label("Done")).style(ui::primary).padding([8, 20]).on_press(wrap(Colour::Done)),
                button(ui::label("Undo")).style(ui::secondary).padding([8, 14]).on_press(wrap(Colour::Undo)),
                button(ui::label("Paperloom's own")).style(ui::ghost).padding([8, 14]).on_press(wrap(Colour::Default)),
            ]
            .spacing(8),
        ]
        .spacing(12);
        ui::modal(content, 340.0, close, t)
    }
}

/// The saturation/brightness square for the current hue.
struct Square<F> {
    hsva: Hsva,
    on_change: F,
}

impl<Message, F: Fn(f32, f32) -> Message> canvas::Program<Message> for Square<F> {
    /// Whether the mouse is held down inside the square.
    type State = bool;

    fn update(&self, dragging: &mut bool, event: &canvas::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<Message>> {
        let at = |position: Point| {
            let s = ((position.x - bounds.x) / bounds.width).clamp(0.0, 1.0);
            let v = 1.0 - ((position.y - bounds.y) / bounds.height).clamp(0.0, 1.0);
            canvas::Action::publish((self.on_change)(s, v)).and_capture()
        };
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => cursor.position_in(bounds).map(|_| {
                *dragging = true;
                at(cursor.position().unwrap_or_default())
            }),
            canvas::Event::Mouse(mouse::Event::CursorMoved { position }) if *dragging => Some(at(*position)),
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if *dragging => {
                *dragging = false;
                None
            }
            _ => None,
        }
    }

    fn draw(&self, _state: &bool, renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let size = frame.size();
        let pure = Hsva { s: 1.0, v: 1.0, a: 1.0, ..self.hsva }.color();
        let across = canvas::gradient::Linear::new(Point::ORIGIN, Point::new(size.width, 0.0)).add_stop(0.0, Color::WHITE).add_stop(1.0, pure);
        frame.fill_rectangle(Point::ORIGIN, size, across);
        let down = canvas::gradient::Linear::new(Point::ORIGIN, Point::new(0.0, size.height)).add_stop(0.0, Color::TRANSPARENT).add_stop(1.0, Color::BLACK);
        frame.fill_rectangle(Point::ORIGIN, size, down);
        let spot = Point::new(self.hsva.s * size.width, (1.0 - self.hsva.v) * size.height);
        frame.stroke(&Path::circle(spot, 7.0), Stroke::default().with_color(Color::WHITE).with_width(2.0));
        frame.stroke(&Path::circle(spot, 8.5), Stroke::default().with_color(Color::BLACK).with_width(1.0));
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, _state: &bool, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }
}

#[derive(Debug, Clone)]
pub enum Fonts {
    Loaded(Vec<String>),
    Search(String),
    Choose(String),
    /// "Paperloom's own".
    Default,
    Close,
}

/// Which of the theme's two fonts is being chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Body,
    Display,
}

pub struct FontPicker {
    pub which: Which,
    families: Option<Vec<String>>,
    search: String,
}

impl FontPicker {
    pub fn new(which: Which) -> Self {
        Self { which, families: None, search: String::new() }
    }

    pub fn update(&mut self, message: &Fonts) {
        match message {
            Fonts::Loaded(list) => self.families = Some(list.clone()),
            Fonts::Search(value) => self.search = value.clone(),
            Fonts::Choose(_) | Fonts::Default | Fonts::Close => {}
        }
    }

    pub fn view<'a, Message: Clone + 'a>(&'a self, wrap: impl Fn(Fonts) -> Message + Copy + 'a, t: theme::Tokens) -> Element<'a, Message> {
        let title = match self.which {
            Which::Body => "Choose the text font",
            Which::Display => "Choose the title font",
        };
        let mut list = column![].spacing(2);
        match &self.families {
            None => list = list.push(text("Reading the fonts on this computer…").size(13).color(t.muted_foreground)),
            Some(families) => {
                let needle = self.search.trim().to_lowercase();
                let shown: Vec<&String> = families.iter().filter(|name| needle.is_empty() || name.to_lowercase().contains(&needle)).collect();
                for name in shown.iter().take(60) {
                    // The name in the normal font (symbol fonts have no readable
                    // name), and a sample in the typeface itself.
                    let face = Font::with_name(theme::family(name));
                    list = list.push(
                        button(
                            row![text((*name).clone()).size(14).color(t.foreground).wrapping(text::Wrapping::None), Space::new().width(Length::Fill), text("Aa Bb 123").font(face).size(17).color(t.muted_foreground).wrapping(text::Wrapping::None)]
                                .spacing(10)
                                .align_y(Alignment::Center),
                        )
                            .width(Length::Fill)
                            .padding([6, 10])
                            .style(ui::ghost)
                            .on_press(wrap(Fonts::Choose((*name).clone()))),
                    );
                }
                if shown.len() > 60 {
                    list = list.push(text(format!("{} more: type in the box to narrow it down.", shown.len() - 60)).size(12).color(t.muted_foreground));
                }
                if shown.is_empty() {
                    list = list.push(text("No font matches that.").size(13).color(t.muted_foreground));
                }
            }
        }
        let content = column![
            text(title).font(theme::display_font()).size(18).color(t.foreground),
            text_input("Search fonts…", &self.search).on_input(move |value| wrap(Fonts::Search(value))).padding(7).size(13),
            container(scrollable(container(list).padding(iced::Padding { right: 10.0, ..iced::Padding::ZERO })).height(Length::Fixed(320.0))).width(Length::Fill),
            row![button(ui::label("Paperloom's own")).style(ui::secondary).padding([8, 14]).on_press(wrap(Fonts::Default)), button(ui::label("Close")).style(ui::ghost).padding([8, 14]).on_press(wrap(Fonts::Close))].spacing(8),
        ]
        .spacing(12);
        ui::modal(content, 420.0, wrap(Fonts::Close), t)
    }
}

/// Keeps `Size` imported for the canvas program's frame maths.
const _: fn(Size) = |_| {};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_round_trips_through_rgb() {
        for hex in ["#9A3412", "#FB923C", "#22CC88", "#000000", "#FFFFFF", "#7F7F7F"] {
            let rgba = paperloom_theme::parse_color(hex).unwrap();
            let back = Hsva::from_rgba(rgba).to_rgba();
            for (a, b) in rgba.iter().zip(back) {
                assert!((a - b).abs() < 0.002, "{hex}: {rgba:?} vs {back:?}");
            }
        }
        assert_eq!(Hsva::from_rgba([1.0, 0.0, 0.0, 1.0]).h, 0.0);
        assert!((Hsva::from_rgba([0.0, 0.0, 1.0, 1.0]).h - 240.0).abs() < 0.01);
    }

    #[test]
    fn typing_a_colour_moves_the_picker() {
        let mut picker = ColourPicker::new("x", [0.0, 0.0, 0.0, 1.0], None, &[]);
        assert!(picker.update(&Colour::Hex("#FF0000".into())));
        assert_eq!(picker.hsva.hex(), "#FF0000");
        assert!(!picker.update(&Colour::Hex("#FF".into())), "half a colour changes nothing");
        assert_eq!(picker.hsva.hex(), "#FF0000");
        assert!(picker.update(&Colour::Square(0.5, 1.0)));
        assert_eq!(picker.hex, picker.hsva.hex());
    }
}
