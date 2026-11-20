//! Editing a theme's file through a form. The file is plain text anyone
//! can open in a notepad; this is the same file with colour fields, sliders
//! and choices, plus a "Raw file" tab with the text itself.
//!
//! The preview applies the draft to the whole window through one function
//! (`Outcome::Preview`), so a live picture of the app can replace it later.

use std::path::PathBuf;

use iced::widget::{button, column, container, pick_list, row, scrollable, slider, text, text_editor, text_input, toggler, Space};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Task};

use paperloom_theme::{Anchor, Colors, Layer, Listed, Nav, NavStyle, Part, Theme, PARTS};

use super::pick::{self, ColourPicker, FontPicker};
use crate::pages::widgets_looks::blocking;
use crate::{theme, ui};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Colours,
    Fonts,
    Shape,
    Layout,
    Backdrop,
    Layers,
    Parts,
    Raw,
}

const TABS: [(Tab, &str); 8] = [
    (Tab::Colours, "Colours"),
    (Tab::Fonts, "Fonts"),
    (Tab::Shape, "Shape"),
    (Tab::Layout, "Layout"),
    (Tab::Backdrop, "Backdrop"),
    (Tab::Layers, "Pictures"),
    (Tab::Parts, "Parts"),
    (Tab::Raw, "Raw file"),
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Number {
    ControlRadius,
    CardRadius,
    PopupRadius,
    BorderWidth,
    Shadow,
    Scale,
    NavWidth,
    MaxWidth,
    BackdropOpacity,
    BackdropAngle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Words {
    Name,
    Author,
    Description,
    BodyFont,
    DisplayFont,
    GradientFrom,
    GradientTo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartField {
    Background,
    Foreground,
    Border,
}

/// Which colour the picker is changing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// One of the general colours (of the mode being edited).
    General(&'static str),
    GradientFrom,
    GradientTo,
    Part(PartField),
}

#[derive(Debug, Clone)]
pub enum Message {
    OpenColor(Target),
    Colour(pick::Colour),
    OpenFont(pick::Which),
    Fonts(pick::Fonts),
    PickFrames(usize),
    TabSelected(Tab),
    /// Edit the dark colours (true) or the light ones.
    Mode(bool),
    Color(&'static str, String),
    Words(Words, String),
    Number(Number, f32),
    Nav(Nav),
    NavStyle(NavStyle),
    Search(bool),
    Title(bool),
    PickBackdrop,
    ClearBackdrop,
    PickFont,
    AddLayer,
    RemoveLayer(usize),
    LayerAnchor(usize, Anchor),
    LayerFront(usize, bool),
    LayerOpacity(usize, f32),
    LayerSize(usize, f32),
    PartSelected(&'static str),
    PartColor(PartField, String),
    PartRadius(f32),
    PartClear,
    Raw(text_editor::Action),
    Preview,
    Save,
    Cancel,
}

pub enum Outcome {
    None,
    Preview(Box<Theme>, Option<PathBuf>),
    Saved(String),
    Closed,
}

const ANCHORS: [Anchor; 9] = [Anchor::TopLeft, Anchor::Top, Anchor::TopRight, Anchor::Left, Anchor::Center, Anchor::Right, Anchor::BottomLeft, Anchor::Bottom, Anchor::BottomRight];

/// `Anchor` with a readable name (for the pick list).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Corner(Anchor);

impl std::fmt::Display for Corner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            Anchor::TopLeft => "Top left",
            Anchor::Top => "Top",
            Anchor::TopRight => "Top right",
            Anchor::Left => "Left",
            Anchor::Center => "Centre",
            Anchor::Right => "Right",
            Anchor::BottomLeft => "Bottom left",
            Anchor::Bottom => "Bottom",
            Anchor::BottomRight => "Bottom right",
        })
    }
}

pub struct State {
    dir: Option<PathBuf>,
    draft: Theme,
    tab: Tab,
    dark: bool,
    part: &'static str,
    raw: text_editor::Content,
    /// The colour picker, while it is open.
    picking: Option<(Target, ColourPicker)>,
    /// The font picker, while it is open.
    fonts: Option<FontPicker>,
    /// Why the raw text or a save was refused.
    error: Option<String>,
    warnings: Vec<String>,
}

impl State {
    pub fn new(listed: Listed) -> Self {
        let dir = listed.dir().map(PathBuf::from);
        let dark = listed.theme.colors.dark.is_some() || listed.theme.colors.light.is_none();
        Self { dir, draft: listed.theme, tab: Tab::Colours, dark, part: PARTS[0].0, raw: text_editor::Content::new(), picking: None, fonts: None, error: None, warnings: Vec::new() }
    }

    fn colors(&mut self) -> &mut Colors {
        if self.dark {
            self.draft.colors.dark.get_or_insert_with(Colors::default)
        } else {
            self.draft.colors.light.get_or_insert_with(Colors::default)
        }
    }

    /// The colour `target` has now (written or default).
    fn current(&self, target: Target) -> ([f32; 4], Option<String>) {
        let palette = self.draft.palette(self.dark);
        let part = self.draft.part.get(self.part);
        let (written, fallback) = match target {
            Target::General(name) => ((if self.dark { &self.draft.colors.dark } else { &self.draft.colors.light }).as_ref().and_then(|colors| colors.get(name)).cloned(), palette.get(name).unwrap_or([0.0, 0.0, 0.0, 1.0])),
            Target::GradientFrom => (self.draft.backdrop.gradient.first().cloned(), palette.background),
            Target::GradientTo => (self.draft.backdrop.gradient.get(1).cloned(), palette.surface),
            Target::Part(PartField::Background) => (part.and_then(|part| part.background.clone()), palette.surface),
            Target::Part(PartField::Foreground) => (part.and_then(|part| part.foreground.clone()), palette.foreground),
            Target::Part(PartField::Border) => (part.and_then(|part| part.border.clone()), palette.border),
        };
        (written.as_deref().and_then(paperloom_theme::parse_color).unwrap_or(fallback), written)
    }

    /// Writes `value` (`None` = Paperloom's own) as the colour `target`.
    fn write_colour(&mut self, target: Target, value: Option<String>) {
        match target {
            Target::General(name) => self.colors().set(name, value),
            Target::GradientFrom | Target::GradientTo => {
                let at = usize::from(target == Target::GradientTo);
                let gradient = &mut self.draft.backdrop.gradient;
                gradient.resize(2, String::new());
                gradient[at] = value.unwrap_or_default();
                if gradient.iter().all(String::is_empty) {
                    gradient.clear();
                }
            }
            Target::Part(field) => {
                let part = self.draft.part.entry(self.part.to_string()).or_default();
                match field {
                    PartField::Background => part.background = value,
                    PartField::Foreground => part.foreground = value,
                    PartField::Border => part.border = value,
                }
            }
        }
    }

    fn target_title(&self, target: Target) -> String {
        match target {
            Target::General(name) => format!("{} ({} mode)", name.replace('_', " "), if self.dark { "dark" } else { "light" }),
            Target::GradientFrom => "Gradient: from".to_string(),
            Target::GradientTo => "Gradient: to".to_string(),
            Target::Part(PartField::Background) => format!("{}: background", self.part),
            Target::Part(PartField::Foreground) => format!("{}: text", self.part),
            Target::Part(PartField::Border) => format!("{}: outline", self.part),
        }
    }

    /// Reads the Raw tab's text into the draft. `false` (with the reason
    /// kept) when it isn't a usable theme.
    fn take_raw(&mut self) -> bool {
        let report = paperloom_theme::check(&self.raw.text(), self.dir.as_deref());
        self.warnings = report.warnings.clone();
        match report.theme.filter(|_| report.errors.is_empty()) {
            Some(mut theme) => {
                // The id is the theme's identity (and its folder): it stays.
                theme.id = self.draft.id.clone();
                self.draft = theme;
                self.error = None;
                true
            }
            None => {
                self.error = Some(report.errors.join(" "));
                false
            }
        }
    }

    /// Copies a picked file into the theme's folder; the path to write in the file.
    fn bring_in(&mut self, folder: &str, kinds: &[&str], title: &str) -> Option<String> {
        let dir = self.dir.clone()?;
        let picked = rfd::FileDialog::new().set_title(title).add_filter("Files", kinds).pick_file()?;
        let name = picked.file_name()?.to_string_lossy().replace(' ', "-");
        if std::fs::create_dir_all(dir.join(folder)).is_err() || std::fs::copy(&picked, dir.join(folder).join(&name)).is_err() {
            self.error = Some(format!("Couldn't copy {} into the theme's folder.", picked.display()));
            return None;
        }
        Some(format!("{folder}/{name}"))
    }

    pub fn update(&mut self, message: Message) -> (Task<Message>, Outcome) {
        // Leaving the Raw tab (or acting from it) reads the text first.
        let on_raw = self.tab == Tab::Raw;
        match message {
            Message::OpenColor(target) => {
                let (current, written) = self.current(target);
                let own: Vec<[f32; 4]> = Colors::FIELDS.iter().filter_map(|(name, _)| self.draft.palette(self.dark).get(name)).collect();
                self.picking = Some((target, ColourPicker::new(self.target_title(target), current, written, &own)));
            }
            Message::Colour(change) => {
                let Some((target, picker)) = self.picking.as_mut() else { return (Task::none(), Outcome::None) };
                let target = *target;
                match &change {
                    pick::Colour::Done => self.picking = None,
                    pick::Colour::Undo => {
                        let before = picker.before.clone();
                        if let Some(colour) = before.as_deref().and_then(paperloom_theme::parse_color) {
                            picker.update(&pick::Colour::Swatch(colour));
                        }
                        self.write_colour(target, before);
                    }
                    pick::Colour::Default => {
                        self.write_colour(target, None);
                        self.picking = None;
                    }
                    other => {
                        if picker.update(other) {
                            let hex = picker.hsva.hex();
                            self.write_colour(target, Some(hex));
                        }
                    }
                }
            }
            Message::OpenFont(which) => {
                self.fonts = Some(FontPicker::new(which));
                return (blocking(paperloom_skin::font_families, |list| Message::Fonts(pick::Fonts::Loaded(list))), Outcome::None);
            }
            Message::Fonts(change) => {
                let Some(picker) = self.fonts.as_mut() else { return (Task::none(), Outcome::None) };
                let which = picker.which;
                match change {
                    pick::Fonts::Choose(name) => {
                        match which {
                            pick::Which::Body => self.draft.fonts.body = name,
                            pick::Which::Display => self.draft.fonts.display = name,
                        }
                        self.fonts = None;
                    }
                    pick::Fonts::Default => {
                        match which {
                            pick::Which::Body => self.draft.fonts.body.clear(),
                            pick::Which::Display => self.draft.fonts.display.clear(),
                        }
                        self.fonts = None;
                    }
                    pick::Fonts::Close => self.fonts = None,
                    other => picker.update(&other),
                }
            }
            Message::PickFrames(index) => {
                if let Some(dir) = self.dir.clone() {
                    if let Some(picked) = rfd::FileDialog::new().set_title("Choose the frames of the animation, in order (PNG)").add_filter("Pictures", &["png"]).pick_files() {
                        let mut frames = Vec::new();
                        let mut picked = picked;
                        picked.sort();
                        for file in picked {
                            let Some(name) = file.file_name().map(|name| name.to_string_lossy().replace(' ', "-")) else { continue };
                            if std::fs::create_dir_all(dir.join("images")).is_ok() && std::fs::copy(&file, dir.join("images").join(&name)).is_ok() {
                                frames.push(format!("images/{name}"));
                            }
                        }
                        if let Some(layer) = self.draft.layer.get_mut(index).filter(|_| !frames.is_empty()) {
                            layer.frames = frames;
                        }
                    }
                }
            }
            Message::TabSelected(tab) => {
                if on_raw && tab != Tab::Raw && !self.take_raw() {
                    return (Task::none(), Outcome::None);
                }
                if tab == Tab::Raw {
                    self.raw = text_editor::Content::with_text(&toml::to_string_pretty(&self.draft).unwrap_or_default());
                    self.error = None;
                }
                self.tab = tab;
            }
            Message::Mode(dark) => self.dark = dark,
            Message::Color(name, value) => {
                let value = value.trim().to_string();
                self.colors().set(name, (!value.is_empty()).then_some(value));
            }
            Message::Words(which, value) => match which {
                Words::Name => self.draft.name = value,
                Words::Author => self.draft.author = value,
                Words::Description => self.draft.description = value,
                Words::BodyFont => self.draft.fonts.body = value,
                Words::DisplayFont => self.draft.fonts.display = value,
                Words::GradientFrom | Words::GradientTo => {
                    let gradient = &mut self.draft.backdrop.gradient;
                    gradient.resize(2, String::new());
                    gradient[usize::from(which == Words::GradientTo)] = value.trim().to_string();
                    if gradient.iter().all(String::is_empty) {
                        gradient.clear();
                    }
                }
            },
            Message::Number(which, value) => {
                let (shape, layout, backdrop) = (&mut self.draft.shape, &mut self.draft.layout, &mut self.draft.backdrop);
                match which {
                    Number::ControlRadius => shape.control_radius = value,
                    Number::CardRadius => shape.card_radius = value,
                    Number::PopupRadius => shape.popup_radius = value,
                    Number::BorderWidth => shape.border_width = value,
                    Number::Shadow => shape.shadow = value,
                    Number::Scale => shape.scale = value,
                    Number::NavWidth => layout.nav_width = value,
                    Number::MaxWidth => layout.content_max_width = if value < 480.0 { 0.0 } else { value },
                    Number::BackdropOpacity => backdrop.opacity = value,
                    Number::BackdropAngle => backdrop.angle = value,
                }
            }
            Message::Nav(nav) => self.draft.layout.nav = nav,
            Message::NavStyle(style) => self.draft.layout.nav_style = style,
            Message::Search(on) => self.draft.layout.search = on,
            Message::Title(on) => self.draft.layout.title = on,
            Message::PickBackdrop => {
                if let Some(path) = self.bring_in("images", &["png"], "Choose a backdrop picture (PNG)") {
                    self.draft.backdrop.image = path;
                }
            }
            Message::ClearBackdrop => self.draft.backdrop.image.clear(),
            Message::PickFont => {
                if let Some(path) = self.bring_in("fonts", &["ttf", "otf"], "Choose a font file") {
                    if !self.draft.fonts.files.contains(&path) {
                        self.draft.fonts.files.push(path);
                    }
                }
            }
            Message::AddLayer => {
                if let Some(path) = self.bring_in("images", &["png"], "Choose a picture (PNG)") {
                    self.draft.layer.push(Layer { image: path, ..Layer::default() });
                }
            }
            Message::RemoveLayer(index) if index < self.draft.layer.len() => {
                self.draft.layer.remove(index);
            }
            Message::RemoveLayer(_) => {}
            Message::LayerAnchor(index, anchor) => {
                if let Some(layer) = self.draft.layer.get_mut(index) {
                    layer.anchor = anchor;
                }
            }
            Message::LayerFront(index, front) => {
                if let Some(layer) = self.draft.layer.get_mut(index) {
                    layer.front = front;
                }
            }
            Message::LayerOpacity(index, opacity) => {
                if let Some(layer) = self.draft.layer.get_mut(index) {
                    layer.opacity = opacity;
                }
            }
            Message::LayerSize(index, width) => {
                if let Some(layer) = self.draft.layer.get_mut(index) {
                    layer.width = if width < 24.0 { 0.0 } else { width };
                }
            }
            Message::PartSelected(part) => self.part = part,
            Message::PartColor(field, value) => {
                let value = value.trim().to_string();
                let value = (!value.is_empty()).then_some(value);
                let part = self.draft.part.entry(self.part.to_string()).or_default();
                match field {
                    PartField::Background => part.background = value,
                    PartField::Foreground => part.foreground = value,
                    PartField::Border => part.border = value,
                }
            }
            Message::PartRadius(radius) => self.draft.part.entry(self.part.to_string()).or_default().radius = Some(radius),
            Message::PartClear => {
                self.draft.part.remove(self.part);
            }
            Message::Raw(action) => self.raw.perform(action),
            Message::Preview => {
                if on_raw && !self.take_raw() {
                    return (Task::none(), Outcome::None);
                }
                return (Task::none(), Outcome::Preview(Box::new(self.tidy()), self.dir.clone()));
            }
            Message::Save => {
                if on_raw && !self.take_raw() {
                    return (Task::none(), Outcome::None);
                }
                let theme = self.tidy();
                // The same check an import gets: a file that can't be used isn't written.
                let report = paperloom_theme::check(&toml::to_string_pretty(&theme).unwrap_or_default(), self.dir.as_deref());
                if !report.errors.is_empty() {
                    self.error = Some(report.errors.join(" "));
                    return (Task::none(), Outcome::None);
                }
                return match paperloom_theme::save(&paperloom_common::themes_dir(), &theme) {
                    Ok(_) => (Task::none(), Outcome::Saved(theme.id)),
                    Err(err) => {
                        self.error = Some(format!("Couldn't save the theme: {err}"));
                        (Task::none(), Outcome::None)
                    }
                };
            }
            Message::Cancel => return (Task::none(), Outcome::Closed),
        }
        (Task::none(), Outcome::None)
    }

    /// The draft without empty leftovers (a part with nothing set, a mode with no colours).
    fn tidy(&self) -> Theme {
        let mut theme = self.draft.clone();
        theme.part.retain(|_, part| *part != Part::default());
        for mode in [&mut theme.colors.dark, &mut theme.colors.light] {
            if mode.as_ref().is_some_and(|colors| *colors == Colors::default()) {
                *mode = None;
            }
        }
        theme
    }

    pub fn view<'a>(&'a self, _window_dark: bool, t: theme::Tokens) -> Element<'a, Message> {
        let tabs = ui::segmented(TABS.iter().map(|(tab, label)| ((*label).to_string(), self.tab == *tab, Message::TabSelected(*tab))).collect(), t);
        let form: Element<'a, Message> = match self.tab {
            Tab::Colours => self.colours(t),
            Tab::Fonts => self.fonts(t),
            Tab::Shape => self.shape(t),
            Tab::Layout => self.layout(t),
            Tab::Backdrop => self.backdrop(t),
            Tab::Layers => self.layers(t),
            Tab::Parts => self.parts(t),
            Tab::Raw => text_editor(&self.raw).on_action(Message::Raw).font(Font::MONOSPACE).size(13).height(Length::Fill).into(),
        };

        let mut side = column![
            text("Editing a theme").size(12).color(t.muted_foreground),
            words("Name", &self.draft.name, Words::Name, t),
            words("Author", &self.draft.author, Words::Author, t),
            words("Description", &self.draft.description, Words::Description, t),
            ui::rule(t),
            row![super::mock(&self.draft, true, 128.0), super::mock(&self.draft, false, 128.0)].spacing(8),
            text("Preview shows the draft on the whole window for ten seconds.").size(12).color(t.muted_foreground),
            button(ui::label("Preview").size(14)).style(ui::secondary).padding([8, 14]).width(Length::Fill).on_press(Message::Preview),
            button(ui::label("Save").size(14)).style(ui::primary).padding([8, 14]).width(Length::Fill).on_press(Message::Save),
            button(ui::label("Close without saving").size(14)).style(ui::ghost).padding([8, 14]).width(Length::Fill).on_press(Message::Cancel),
        ]
        .spacing(10);
        if let Some(error) = &self.error {
            side = side.push(text(error.clone()).size(13).color(t.destructive));
        }
        for warning in self.warnings.iter().take(4) {
            side = side.push(text(warning.clone()).size(12).color(t.muted_foreground));
        }
        if let Some(dir) = &self.dir {
            side = side.push(text(format!("File: {}", dir.join("theme.toml").display())).size(11).color(t.muted_foreground));
        }

        let form: Element<'a, Message> = if self.tab == Tab::Raw { form } else { scrollable(container(form).padding(iced::Padding { right: 14.0, ..iced::Padding::ZERO })).height(Length::Fill).into() };
        let main = column![scrollable(tabs).direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::hidden())), form].spacing(14).height(Length::Fill);
        row![
            container(panel(scrollable(side.padding(iced::Padding { right: 10.0, ..iced::Padding::ZERO })).height(Length::Fill).into(), t)).width(Length::Fixed(300.0)).height(Length::Fill),
            container(panel(main.into(), t)).width(Length::Fill).height(Length::Fill),
        ]
        .spacing(16)
        .height(Length::Fill)
        .into()
    }

    /// The colour or font picker, over the editor.
    pub fn overlay<'a>(&'a self, t: theme::Tokens) -> Option<Element<'a, Message>> {
        if let Some(fonts) = &self.fonts {
            return Some(fonts.view(Message::Fonts, t));
        }
        let (_, picker) = self.picking.as_ref()?;
        Some(picker.view(Message::Colour, Message::Colour(pick::Colour::Done), t))
    }

    fn colours<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let colors = if self.dark { &self.draft.colors.dark } else { &self.draft.colors.light };
        let palette = self.draft.palette(self.dark);
        let mut rows = column![
            ui::segmented(vec![("Dark colours".to_string(), self.dark, Message::Mode(true)), ("Light colours".to_string(), !self.dark, Message::Mode(false))], t),
            text("Write a colour as #RRGGBB (or #RRGGBBAA for see-through). An empty field keeps Paperloom's own colour.").size(12).color(t.muted_foreground),
        ]
        .spacing(10);
        for (name, about) in Colors::FIELDS {
            let written = colors.as_ref().and_then(|colors| colors.get(name)).cloned().unwrap_or_default();
            let shown = palette.get(name).unwrap_or([0.0, 0.0, 0.0, 1.0]);
            let valid = written.is_empty() || paperloom_theme::parse_color(&written).is_some();
            rows = rows.push(color_row(name, about, written, shown, valid, Message::OpenColor(Target::General(name)), move |value| Message::Color(name, value), t));
        }
        rows.into()
    }

    fn fonts<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let font_row = |label: &'static str, hint: &'static str, name: &str, which: pick::Which, field: Words| {
            let shown = if name.is_empty() { "Paperloom's own".to_string() } else { name.to_string() };
            let face = if name.is_empty() { theme::body_font() } else { Font::with_name(theme::family(name)) };
            column![
                text(label).size(13).color(t.foreground),
                row![
                    container(text(shown).font(face).size(18).color(t.foreground)).padding([8, 12]).width(Length::Fill).style(move |_theme| container::Style { background: Some(Background::Color(t.muted)), border: Border { radius: theme::radius_control().into(), ..Border::default() }, ..container::Style::default() }),
                    button(ui::label("Choose a font").size(13)).style(ui::secondary).padding([9, 14]).on_press(Message::OpenFont(which)),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
                row![text("or type its name").size(12).color(t.muted_foreground), text_input("", name).on_input(move |value| Message::Words(field, value)).padding(5).size(12).width(Length::Fixed(220.0))].spacing(8).align_y(Alignment::Center),
                text(hint).size(12).color(t.muted_foreground),
            ]
            .spacing(6)
        };
        let mut rows = column![
            font_row("Titles", "Page titles and big text.", &self.draft.fonts.display, pick::Which::Display, Words::DisplayFont),
            font_row("Text", "Everything else. A different text font shows after Paperloom restarts.", &self.draft.fonts.body, pick::Which::Body, Words::BodyFont),
            ui::rule(t),
            text("To bring a font along with the theme (so it works on any computer), add its file.").size(12).color(t.muted_foreground),
            button(ui::label("Add a font file").size(13)).style(ui::secondary).padding([7, 12]).on_press_maybe(self.dir.is_some().then_some(Message::PickFont)),
        ]
        .spacing(14);
        for file in &self.draft.fonts.files {
            rows = rows.push(text(file.clone()).size(13).font(Font::MONOSPACE).color(t.foreground));
        }
        rows.into()
    }

    fn shape<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let shape = self.draft.shape;
        column![
            number("Buttons and inputs: corner roundness", shape.control_radius, 0.0..=24.0, 1.0, Number::ControlRadius, "px", t),
            number("Cards and panels: corner roundness", shape.card_radius, 0.0..=32.0, 1.0, Number::CardRadius, "px", t),
            number("Pop-ups: corner roundness", shape.popup_radius, 0.0..=40.0, 1.0, Number::PopupRadius, "px", t),
            number("Outline thickness", shape.border_width, 0.0..=4.0, 0.5, Number::BorderWidth, "px", t),
            number("Shadow strength", shape.shadow, 0.0..=3.0, 0.1, Number::Shadow, "×", t),
            number("Size of everything", shape.scale, 0.7..=1.6, 0.05, Number::Scale, "×", t),
        ]
        .spacing(14)
        .into()
    }

    fn layout<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let layout = self.draft.layout;
        column![
            text("Where the navigation sits").size(13).color(t.foreground),
            ui::segmented(Nav::ALL.iter().map(|nav| (nav.to_string(), layout.nav == *nav, Message::Nav(*nav))).collect(), t),
            text("How it shows").size(13).color(t.foreground),
            ui::segmented(NavStyle::ALL.iter().map(|style| (style.to_string(), layout.nav_style == *style, Message::NavStyle(*style))).collect(), t),
            number("Width of a side navigation with labels", layout.nav_width, 150.0..=360.0, 10.0, Number::NavWidth, "px", t),
            toggler(layout.search).label("Search box in the top bar").size(18).text_size(13).on_toggle(Message::Search),
            toggler(layout.title).label("Page title in the top bar").size(18).text_size(13).on_toggle(Message::Title),
            number("Widest the page may get (far left = the whole window)", layout.content_max_width.max(470.0), 470.0..=2400.0, 10.0, Number::MaxWidth, "px", t),
            text("A narrow window always folds the navigation into the top bar, so every page stays reachable.").size(12).color(t.muted_foreground),
        ]
        .spacing(12)
        .into()
    }

    fn backdrop<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let backdrop = &self.draft.backdrop;
        let stop = |index: usize| backdrop.gradient.get(index).cloned().unwrap_or_default();
        let swatch = |written: &str| paperloom_theme::parse_color(written).unwrap_or([0.0, 0.0, 0.0, 0.0]);
        let (from, to) = (stop(0), stop(1));
        let picture: Element<'a, Message> = if backdrop.image.is_empty() {
            text("No picture").size(13).color(t.muted_foreground).into()
        } else {
            row![text(backdrop.image.clone()).size(13).font(Font::MONOSPACE).color(t.foreground), button(ui::label("Take it out").size(12)).style(ui::ghost).padding([4, 8]).on_press(Message::ClearBackdrop)].spacing(8).align_y(Alignment::Center).into()
        };
        column![
            text("Behind the pages: a gradient, a picture, or both. Give the Background and Surface colours some see-through (#RRGGBBAA) to let it show.").size(12).color(t.muted_foreground),
            color_row("Gradient: from", "first colour (click it to pick)", from.clone(), swatch(&from), from.is_empty() || paperloom_theme::parse_color(&from).is_some(), Message::OpenColor(Target::GradientFrom), |value| Message::Words(Words::GradientFrom, value), t),
            color_row("Gradient: to", "last colour (click it to pick)", to.clone(), swatch(&to), to.is_empty() || paperloom_theme::parse_color(&to).is_some(), Message::OpenColor(Target::GradientTo), |value| Message::Words(Words::GradientTo, value), t),
            number("Gradient direction", backdrop.angle, 0.0..=360.0, 5.0, Number::BackdropAngle, "°", t),
            ui::rule(t),
            picture,
            button(ui::label("Choose a picture").size(13)).style(ui::secondary).padding([7, 12]).on_press_maybe(self.dir.is_some().then_some(Message::PickBackdrop)),
            number("Picture opacity", backdrop.opacity, 0.0..=1.0, 0.05, Number::BackdropOpacity, "", t),
        ]
        .spacing(12)
        .into()
    }

    fn layers<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let mut rows = column![
            text("Pictures placed in the window, behind the pages or in front of them (pictures in front never take clicks). An animation is several frames: list them as “frames” in the Raw file tab.").size(12).color(t.muted_foreground),
            button(ui::label("Add a picture").size(13)).style(ui::secondary).padding([7, 12]).on_press_maybe(self.dir.is_some().then_some(Message::AddLayer)),
        ]
        .spacing(12);
        for (index, layer) in self.draft.layer.iter().enumerate() {
            let name = if layer.frames.is_empty() { layer.image.clone() } else { format!("{} frames, {} a second", layer.frames.len(), layer.fps) };
            let corners: Vec<Corner> = ANCHORS.iter().map(|anchor| Corner(*anchor)).collect();
            let card = column![
                row![text(name).size(13).font(Font::MONOSPACE).color(t.foreground).width(Length::Fill), button(ui::label("Remove").size(12)).style(ui::danger).padding([4, 10]).on_press(Message::RemoveLayer(index))].spacing(8).align_y(Alignment::Center),
                row![
                    text("Place").size(13).color(t.muted_foreground),
                    pick_list(corners, Some(Corner(layer.anchor)), move |corner: Corner| Message::LayerAnchor(index, corner.0)).text_size(13).padding(6),
                    toggler(layer.front).label("In front of the pages").size(18).text_size(13).on_toggle(move |front| Message::LayerFront(index, front)),
                ]
                .spacing(12)
                .align_y(Alignment::Center)
                .wrap(),
                button(ui::label(if layer.frames.is_empty() { "Make it an animation: choose its frames" } else { "Choose different frames" }).size(12)).style(ui::secondary).padding([5, 10]).on_press(Message::PickFrames(index)),
                labelled_slider("Opacity", layer.opacity, 0.0..=1.0, 0.05, move |value| Message::LayerOpacity(index, value), format!("{:.0}%", layer.opacity * 100.0), t),
                labelled_slider("Width (far left = its own size)", layer.width.max(20.0), 20.0..=1600.0, 4.0, move |value| Message::LayerSize(index, value), if layer.width < 24.0 { "own size".to_string() } else { format!("{:.0} px", layer.width) }, t),
            ]
            .spacing(10);
            rows = rows.push(container(card).padding(12).width(Length::Fill).style(move |_theme| container::Style { background: Some(Background::Color(t.muted)), border: Border { radius: theme::radius_card().into(), ..Border::default() }, ..container::Style::default() }));
        }
        rows.into()
    }

    fn parts<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let part = self.draft.part.get(self.part).cloned().unwrap_or_default();
        let palette = self.draft.palette(self.dark);
        let field = |label: &'static str, written: &Option<String>, fallback: paperloom_theme::Rgba, which: PartField| {
            let written = written.clone().unwrap_or_default();
            let shown = paperloom_theme::parse_color(&written).unwrap_or(fallback);
            let valid = written.is_empty() || paperloom_theme::parse_color(&written).is_some();
            color_row(label, "empty = the theme's general colour", written, shown, valid, Message::OpenColor(Target::Part(which)), move |value| Message::PartColor(which, value), t)
        };
        let about = PARTS.iter().find(|(name, _)| *name == self.part).map(|(_, about)| *about).unwrap_or_default();
        let mut picker = row![].spacing(4);
        for (name, _) in PARTS {
            let changed = self.draft.part.contains_key(name);
            picker = picker.push(ui::segment(ui::label(if changed { format!("{name} •") } else { name.to_string() }).size(12).into(), self.part == name, Message::PartSelected(name), t));
        }
        column![
            text("Give one part of the window its own colours and corners. Everything else keeps the theme's general look.").size(12).color(t.muted_foreground),
            picker.wrap().vertical_spacing(4),
            text(format!("{}: {about}", self.part)).size(13).color(t.foreground),
            field("Background", &part.background, palette.surface, PartField::Background),
            field("Text", &part.foreground, palette.foreground, PartField::Foreground),
            field("Outline", &part.border, palette.border, PartField::Border),
            labelled_slider("Corner roundness", part.radius.unwrap_or(0.0), 0.0..=40.0, 1.0, Message::PartRadius, part.radius.map_or("the theme's".to_string(), |radius| format!("{radius:.0} px")), t),
            button(ui::label("Back to the theme's general look").size(13)).style(ui::secondary).padding([7, 12]).on_press(Message::PartClear),
        ]
        .spacing(12)
        .into()
    }
}

fn panel<'a>(content: Element<'a, Message>, t: theme::Tokens) -> Element<'a, Message> {
    container(content)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme| container::Style { background: Some(Background::Color(t.surface)), border: Border { color: t.border, width: theme::border_width(), radius: theme::radius_card().into() }, ..container::Style::default() })
        .into()
}

fn words<'a>(label: &'a str, value: &str, which: Words, t: theme::Tokens) -> Element<'a, Message> {
    column![text(label).size(12).color(t.muted_foreground), text_input("", value).on_input(move |value| Message::Words(which, value)).padding(7).size(13)].spacing(4).into()
}

/// A swatch, what the colour is for, and its hex field (red outline while unreadable).
#[allow(clippy::too_many_arguments)]
fn color_row<'a>(name: &'a str, about: &'a str, written: String, shown: paperloom_theme::Rgba, valid: bool, open: Message, on_input: impl Fn(String) -> Message + 'a, t: theme::Tokens) -> Element<'a, Message> {
    let fill = Color::from_rgba(shown[0], shown[1], shown[2], shown[3]);
    // Click the colour to pick another: the picker opens over the editor.
    let swatch = button(Space::new()).width(34).height(34).padding(0).on_press(open).style(move |_theme, status| button::Style {
        background: Some(Background::Color(fill)),
        border: Border { color: if !valid { t.destructive } else if matches!(status, button::Status::Hovered | button::Status::Pressed) { t.primary } else { t.border }, width: if valid { 1.5 } else { 2.0 }, radius: theme::radius_control().into() },
        ..button::Style::default()
    });
    row![
        swatch,
        column![text(name.replace('_', " ")).size(13).color(t.foreground), text(about).size(11).color(t.muted_foreground)].spacing(1).width(Length::Fill),
        text_input(&paperloom_theme::to_hex(shown), &written).on_input(on_input).padding(6).size(13).font(Font::MONOSPACE).width(Length::Fixed(130.0)),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .into()
}

fn labelled_slider<'a>(label: &'a str, value: f32, range: std::ops::RangeInclusive<f32>, step: f32, on_change: impl Fn(f32) -> Message + 'a, shown: String, t: theme::Tokens) -> Element<'a, Message> {
    column![
        row![text(label).size(13).color(t.foreground).width(Length::Fill), text(shown).size(13).font(Font::MONOSPACE).color(t.muted_foreground)].spacing(8),
        slider(range, value, on_change).step(step),
    ]
    .spacing(4)
    .into()
}

fn number<'a>(label: &'a str, value: f32, range: std::ops::RangeInclusive<f32>, step: f32, which: Number, unit: &str, t: theme::Tokens) -> Element<'a, Message> {
    let shown = if step < 1.0 { format!("{value:.2} {unit}") } else { format!("{value:.0} {unit}") };
    labelled_slider(label, value, range, step, move |value| Message::Number(which, value), shown, t)
}
