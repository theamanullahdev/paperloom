//! Edit layout: a scaled picture of the screen with every placed widget on it.
//! Drag a widget to move it; it snaps to the edges and the middle, can't leave
//! the screen, and is saved when you let go. The same on every platform, and
//! a widget can't get lost off-screen.

use iced::mouse;
use iced::widget::canvas::{self, Action, Frame, Geometry, Path, Stroke, Text};
use iced::{Color, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme};

use paperloom_common::widgets::{entry, Placed};

use super::widgets::Message;
use super::widgets_looks::{size_of as natural_size, skin_of, Previews};
use crate::theme;

/// How close (as a fraction of the screen) an edge must be to snap.
const SNAP: f32 = 0.012;

pub fn view<'a>(placed: &'a [Placed], selected: Option<u32>, screen: (u32, u32), previews: &'a Previews, max_height: f32, t: theme::Tokens) -> Element<'a, Message> {
    let ratio = screen.0 as f32 / screen.1.max(1) as f32;
    iced::widget::responsive(move |size| {
        let width = size.width.min(960.0);
        let height = (width / ratio).min(max_height);
        let width = height * ratio;
        iced::widget::container(canvas::Canvas::new(Layout { placed, selected, screen, previews, t }).width(Length::Fixed(width)).height(Length::Fixed(height)))
            .center_x(Length::Fill)
            .into()
    })
    .into()
}

struct Layout<'a> {
    placed: &'a [Placed],
    selected: Option<u32>,
    screen: (u32, u32),
    previews: &'a Previews,
    t: theme::Tokens,
}

#[derive(Default)]
struct Drag {
    /// The widget being dragged and where inside it the pointer grabbed it,
    /// in fractions of the screen.
    grabbed: Option<(u32, f32, f32)>,
}

impl Layout<'_> {
    /// A widget's size as a fraction of the screen.
    fn size_of(&self, placed: &Placed) -> (f32, f32) {
        let (w, h) = natural_size(placed);
        (w as f32 * placed.scale / self.screen.0 as f32, h as f32 * placed.scale / self.screen.1 as f32)
    }

    fn at(&self, bounds: Rectangle, cursor: Point) -> (f32, f32) {
        ((cursor.x - bounds.x) / bounds.width, (cursor.y - bounds.y) / bounds.height)
    }
}

fn snap(value: f32, targets: &[f32]) -> f32 {
    targets.iter().copied().find(|target| (value - target).abs() < SNAP).unwrap_or(value)
}

impl canvas::Program<Message> for Layout<'_> {
    type State = Drag;

    fn update(&self, state: &mut Drag, event: &Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<Action<Message>> {
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let position = cursor.position_in(bounds)?;
                let (fx, fy) = (position.x / bounds.width, position.y / bounds.height);
                // The widget drawn last is on top.
                let hit = self.placed.iter().rev().find(|placed| {
                    let (w, h) = self.size_of(placed);
                    fx >= placed.x && fx <= placed.x + w && fy >= placed.y && fy <= placed.y + h
                })?;
                state.grabbed = Some((hit.id, fx - hit.x, fy - hit.y));
                Some(Action::publish(Message::Select(hit.id)).and_capture())
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let (id, grab_x, grab_y) = state.grabbed?;
                let position = cursor.position()?;
                let placed = self.placed.iter().find(|placed| placed.id == id)?;
                let (w, h) = self.size_of(placed);
                let (fx, fy) = self.at(bounds, position);
                let (max_x, max_y) = ((1.0 - w).clamp(0.0, 0.95), (1.0 - h).clamp(0.0, 0.95));
                let x = snap(fx - grab_x, &[0.0, max_x, (1.0 - w) / 2.0]).clamp(0.0, max_x);
                let y = snap(fy - grab_y, &[0.0, max_y, (1.0 - h) / 2.0]).clamp(0.0, max_y);
                Some(Action::publish(Message::Moved(id, x, y)).and_capture())
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.grabbed.take()?;
                Some(Action::publish(Message::Dropped).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, _state: &Drag, renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (width, height) = (bounds.width, bounds.height);
        // The user's own wallpaper, so the picture reads like their screen.
        match &self.previews.wallpaper {
            Some(wallpaper) => frame.draw_image(Rectangle::new(Point::ORIGIN, bounds.size()), wallpaper),
            None => frame.fill_rectangle(Point::ORIGIN, bounds.size(), Color { a: 0.5, ..self.t.muted }),
        }
        let faint = Stroke::default().with_color(Color { a: 0.5, ..self.t.border }).with_width(1.0);
        for third in [1.0 / 3.0, 2.0 / 3.0] {
            frame.stroke(&Path::line(Point::new(width * third, 0.0), Point::new(width * third, height)), faint);
            frame.stroke(&Path::line(Point::new(0.0, height * third), Point::new(width, height * third)), faint);
        }
        frame.stroke_rectangle(Point::ORIGIN, bounds.size(), Stroke::default().with_color(self.t.border).with_width(2.0));

        for placed in self.placed {
            let (w, h) = self.size_of(placed);
            let (origin, size) = (Point::new(placed.x * width, placed.y * height), Size::new(w * width, h * height));
            let chosen = self.selected == Some(placed.id);
            let card = Path::rounded_rectangle(origin, size, 4.0.into());
            match skin_of(placed).and_then(|skin| self.previews.bare(&skin.id, &placed.kind)) {
                // The widget as it is drawn on the desktop.
                Some(preview) => frame.draw_image(Rectangle::new(origin, size), canvas::Image::new(preview.clone()).opacity(placed.opacity)),
                None => {
                    frame.fill(&card, Color { a: 0.9, ..self.t.surface });
                    frame.fill_text(Text {
                        content: entry(&placed.kind).map_or(placed.kind.clone(), |entry| entry.name.to_string()),
                        position: Point::new(origin.x + size.width / 2.0, origin.y + size.height / 2.0),
                        color: self.t.foreground,
                        size: 13.0.into(),
                        align_x: iced::alignment::Horizontal::Center.into(),
                        align_y: iced::alignment::Vertical::Center,
                        ..Text::default()
                    });
                }
            }
            if chosen {
                frame.stroke(&card, Stroke::default().with_color(self.t.primary).with_width(2.0));
            }
        }
        // A widget in a stack: the whole stack is outlined, since it moves as one.
        if let Some(stack) = self.selected.and_then(|id| self.placed.iter().find(|placed| placed.id == id)).map(|placed| placed.stack).filter(|stack| *stack != 0) {
            let (mut left, mut top, mut right, mut bottom) = (f32::MAX, f32::MAX, 0.0_f32, 0.0_f32);
            for placed in self.placed.iter().filter(|placed| placed.stack == stack) {
                let (w, h) = self.size_of(placed);
                (left, top) = (left.min(placed.x), top.min(placed.y));
                (right, bottom) = (right.max(placed.x + w), bottom.max(placed.y + h));
            }
            if right > left {
                let pad = 4.0;
                let rect = Path::rounded_rectangle(Point::new(left * width - pad, top * height - pad), Size::new((right - left) * width + pad * 2.0, (bottom - top) * height + pad * 2.0), 6.0.into());
                frame.stroke(&rect, Stroke::default().with_color(self.t.primary).with_width(1.5));
            }
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, state: &Drag, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if state.grabbed.is_some() {
            return mouse::Interaction::Grabbing;
        }
        let Some(position) = cursor.position_in(bounds) else { return mouse::Interaction::default() };
        let (fx, fy) = (position.x / bounds.width, position.y / bounds.height);
        let over = self.placed.iter().any(|placed| {
            let (w, h) = self.size_of(placed);
            fx >= placed.x && fx <= placed.x + w && fy >= placed.y && fy <= placed.y + h
        });
        if over {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}
