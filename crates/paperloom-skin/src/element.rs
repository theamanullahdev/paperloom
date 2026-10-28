//! The skin file format: a name, the widget kinds it can show, a size, and a
//! list of elements drawn in order. Coordinates are pixels from the top-left at
//! natural size; angles are degrees clockwise from twelve o'clock.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Skin {
    /// `kind/name`, unique, e.g. `clock/neon`.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub author: String,
    /// The widget kinds this look can be used for (`clock`, `cpu`, …).
    pub kinds: Vec<String>,
    /// Natural width and height.
    pub size: [f32; 2],
    #[serde(default)]
    pub element: Vec<Element>,
    /// Shows seconds, so it must be redrawn every second (computed on load).
    #[serde(skip)]
    pub uses_seconds: bool,
}

impl Skin {
    pub fn parse(source: &str) -> Result<Self, toml::de::Error> {
        let mut skin: Skin = toml::from_str(source)?;
        skin.uses_seconds = format!("{:?}", skin.element).contains("second");
        Ok(skin)
    }
}

fn accent() -> String {
    "$accent".into()
}
fn track() -> String {
    "$track".into()
}
fn foreground() -> String {
    "$fg".into()
}
fn one() -> f32 {
    1.0
}
fn left() -> String {
    "left".into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Element {
    Rect(Rect),
    Circle(Circle),
    Text(TextElement),
    Bar(Bar),
    Arc(Arc),
    Hand(Hand),
    Ticks(Ticks),
}

impl Element {
    /// Drawn only when this variable exists (a network widget has no fraction to fill a bar with).
    pub fn show_if(&self) -> Option<&str> {
        match self {
            Element::Rect(e) => e.show_if.as_deref(),
            Element::Circle(e) => e.show_if.as_deref(),
            Element::Text(e) => e.show_if.as_deref(),
            Element::Bar(e) => e.show_if.as_deref(),
            Element::Arc(e) => e.show_if.as_deref(),
            Element::Hand(e) => e.show_if.as_deref(),
            Element::Ticks(e) => e.show_if.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    #[serde(default)]
    pub radius: f32,
    pub fill: Option<String>,
    pub stroke: Option<String>,
    #[serde(default = "one")]
    pub stroke_width: f32,
    pub show_if: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Circle {
    pub cx: f32,
    pub cy: f32,
    pub r: f32,
    pub fill: Option<String>,
    pub stroke: Option<String>,
    #[serde(default = "one")]
    pub stroke_width: f32,
    pub show_if: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TextElement {
    /// May contain `{variables}`.
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub mono: bool,
    #[serde(default)]
    pub serif: bool,
    #[serde(default = "foreground")]
    pub color: String,
    /// `left`, `center` or `right`: which side of `x` the text sits on.
    #[serde(default = "left")]
    pub align: String,
    pub show_if: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Bar {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// The variable (0–1) that fills it.
    pub value: String,
    #[serde(default = "accent")]
    pub fill: String,
    #[serde(default = "track")]
    pub track: String,
    pub show_if: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Arc {
    pub cx: f32,
    pub cy: f32,
    pub r: f32,
    #[serde(default = "six")]
    pub width: f32,
    /// Degrees clockwise from twelve o'clock where the arc begins.
    #[serde(default)]
    pub start: f32,
    #[serde(default = "full")]
    pub sweep: f32,
    /// The variable (0–1) that fills it; absent = only the track is drawn.
    pub value: Option<String>,
    #[serde(default = "accent")]
    pub fill: String,
    #[serde(default = "track")]
    pub track: String,
    pub show_if: Option<String>,
}

fn six() -> f32 {
    6.0
}
fn full() -> f32 {
    360.0
}

#[derive(Debug, Clone, Deserialize)]
pub struct Hand {
    pub cx: f32,
    pub cy: f32,
    pub length: f32,
    #[serde(default = "three")]
    pub width: f32,
    /// The variable holding the angle in degrees.
    pub angle: String,
    #[serde(default = "foreground")]
    pub color: String,
    /// How far it reaches behind the centre.
    #[serde(default)]
    pub tail: f32,
    pub show_if: Option<String>,
}

fn three() -> f32 {
    3.0
}

#[derive(Debug, Clone, Deserialize)]
pub struct Ticks {
    pub cx: f32,
    pub cy: f32,
    pub outer: f32,
    pub inner: f32,
    #[serde(default = "twelve")]
    pub count: u32,
    #[serde(default = "two")]
    pub width: f32,
    #[serde(default = "foreground")]
    pub color: String,
    /// Every this-many ticks is a long one (0 = none).
    #[serde(default)]
    pub big_every: u32,
    pub big_inner: Option<f32>,
    pub show_if: Option<String>,
}

fn twelve() -> u32 {
    12
}
fn two() -> f32 {
    2.0
}
