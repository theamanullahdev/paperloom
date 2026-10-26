//! Small shared canvas charts (scrolling line graphs) used by the
//! Downloads page's connection meter and the Usage page.

use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Theme};

use crate::theme;

/// What a chart's numbers mean, so its y-axis can print them.
#[derive(Clone, Copy)]
pub enum Unit {
    /// 0–100, shown as `12%`.
    Percent,
    /// A size, shown as `183 MB`.
    Bytes,
    /// A speed, shown as `256 KB/s`.
    BytesPerSecond,
}

impl Unit {
    fn label(self, value: f32) -> String {
        match self {
            Unit::Percent if value < 10.0 => format!("{value:.1}%"),
            Unit::Percent => format!("{value:.0}%"),
            Unit::Bytes => human(value),
            Unit::BytesPerSecond => format!("{}/s", human(value)),
        }
    }
}

fn human(bytes: f32) -> String {
    const KB: f32 = 1024.0;
    if bytes >= KB * KB * KB {
        format!("{:.1} GB", bytes / (KB * KB * KB))
    } else if bytes >= KB * KB {
        format!("{:.0} MB", bytes / (KB * KB))
    } else if bytes >= KB {
        format!("{:.0} KB", bytes / KB)
    } else {
        format!("{bytes:.0} B")
    }
}

/// Room on the left for the y-axis numbers.
const GUTTER: f32 = 64.0;

/// One line on a chart; newest sample last.
pub struct Series {
    pub samples: Vec<f32>,
    pub color: Color,
}

/// A line graph with a faint grid. All series share one scale: the
/// largest value in view, never below `floor` (so idle noise doesn't fill
/// the whole graph). A series with fewer than `window` samples is pinned
/// to the right edge so the graph "scrolls in" from the right.
pub fn line_chart<'a, M: 'a>(series: Vec<Series>, t: theme::Tokens, unit: Unit, floor: f32, window: usize, height: f32) -> Element<'a, M> {
    canvas::Canvas::new(Chart { series, t, unit, floor, window: window.max(2) }).width(Length::Fill).height(Length::Fixed(height)).into()
}

struct Chart {
    series: Vec<Series>,
    t: theme::Tokens,
    unit: Unit,
    floor: f32,
    window: usize,
}

impl<M> canvas::Program<M> for Chart {
    type State = ();

    fn draw(&self, _state: &(), renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (width, height) = (bounds.width, bounds.height);

        let plot = width - GUTTER;
        frame.fill_rectangle(Point::new(GUTTER, 0.0), iced::Size::new(plot, height), Color { a: 0.35, ..self.t.muted });

        let top = self.series.iter().flat_map(|series| series.samples.iter().copied()).fold(self.floor, f32::max) * 1.1;
        let y_at = |value: f32| height - (value / top).clamp(0.0, 1.0) * (height - 2.0) - 1.0;

        // Gridlines with their values: the top, three steps down, and zero.
        for step in 0..=4 {
            let y = height * step as f32 / 4.0;
            if (1..4).contains(&step) {
                frame.stroke(&Path::line(Point::new(GUTTER, y), Point::new(width, y)), Stroke::default().with_color(self.t.border).with_width(1.0));
            }
            let value = top * (1.0 - step as f32 / 4.0);
            frame.fill_text(canvas::Text {
                content: self.unit.label(value),
                position: Point::new(GUTTER - 8.0, y),
                color: self.t.muted_foreground,
                size: 11.0.into(),
                align_x: iced::alignment::Horizontal::Right.into(),
                align_y: match step {
                    0 => iced::alignment::Vertical::Top,
                    4 => iced::alignment::Vertical::Bottom,
                    _ => iced::alignment::Vertical::Center,
                },
                ..canvas::Text::default()
            });
        }

        for series in &self.series {
            let count = series.samples.len();
            if count < 2 {
                continue;
            }
            let offset = plot * (1.0 - count as f32 / self.window as f32);
            let x_at = |index: usize| GUTTER + plot * index as f32 / (self.window - 1) as f32 + offset;

            let line = Path::new(|builder| {
                builder.move_to(Point::new(x_at(0), y_at(series.samples[0])));
                for (index, value) in series.samples.iter().enumerate().skip(1) {
                    builder.line_to(Point::new(x_at(index), y_at(*value)));
                }
            });
            let area = Path::new(|builder| {
                builder.move_to(Point::new(x_at(0), height));
                for (index, value) in series.samples.iter().enumerate() {
                    builder.line_to(Point::new(x_at(index), y_at(*value)));
                }
                builder.line_to(Point::new(x_at(count - 1), height));
                builder.close();
            });
            frame.fill(&area, Color { a: 0.16, ..series.color });
            frame.stroke(&line, Stroke::default().with_color(series.color).with_width(2.0));
        }
        vec![frame.into_geometry()]
    }
}
