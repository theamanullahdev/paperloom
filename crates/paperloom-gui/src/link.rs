//! `paperloom://install/<kind>/<id>`: the "Open in Paperloom" button on the
//! website. A link only names something in the Paperloom library; the app
//! looks it up in the library's own index (never a link from the URL),
//! shows what it is, and installs it only when the person says Add.
//!
//! One pop-up for the whole app, opened by `App::open_link`.

use iced::widget::{button, column, image, row, text};
use iced::{Alignment, Element, Length, Task};
use paperloom_fetch::looks::{self, Pack};

use crate::pages::widgets_looks::blocking;
use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Look,
    Stack,
    Theme,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Look => "Widget look",
            Kind::Stack => "Stack of widgets",
            Kind::Theme => "App theme",
        }
    }
}

/// `(kind, id)` of a `paperloom://install/<kind>/<id>` link; `None` for anything else.
pub fn parse(url: &str) -> Option<(Kind, String)> {
    let rest = url.trim().strip_prefix("paperloom://install/")?;
    let (kind, id) = rest.split_once('/')?;
    let kind = match kind {
        "look" => Kind::Look,
        "stack" => Kind::Stack,
        "theme" => Kind::Theme,
        _ => return None,
    };
    let id = percent_decode(id.trim_end_matches('/'))?;
    let safe = !id.is_empty() && id.len() <= 80 && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/')) && !id.contains("..");
    safe.then_some((kind, id))
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[derive(Debug, Clone)]
pub struct Found {
    pack: Pack,
    base: String,
    /// All catalogue looks, so a stack can bring the ones it uses.
    all_looks: Vec<Pack>,
    preview: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Result<Box<Found>, String>),
    Add,
    Installed(Result<(), String>),
    UseTheme,
    Close,
}

enum Stage {
    Loading,
    Ready(Box<Ready>),
    Installing(Box<Ready>),
    Done(Box<Ready>),
    Failed(String),
}

struct Ready {
    pack: Pack,
    base: String,
    all_looks: Vec<Pack>,
    preview: Option<image::Handle>,
}

pub struct State {
    kind: Kind,
    id: String,
    stage: Stage,
}

/// What the app should do after a message.
pub enum Outcome {
    Nothing,
    Close,
    UseTheme(String),
}

impl State {
    /// Starts looking `id` up in the library.
    pub fn open(kind: Kind, id: String) -> (Self, Task<Message>) {
        let wanted = id.clone();
        let task = blocking(move || find(kind, &wanted), Message::Loaded);
        (State { kind, id, stage: Stage::Loading }, task)
    }

    pub fn update(&mut self, message: Message) -> (Task<Message>, Outcome) {
        match message {
            Message::Loaded(Ok(found)) => {
                let found = *found;
                let preview = found.preview.map(image::Handle::from_bytes);
                self.stage = Stage::Ready(Box::new(Ready { pack: found.pack, base: found.base, all_looks: found.all_looks, preview }));
            }
            Message::Loaded(Err(why)) => self.stage = Stage::Failed(why),
            Message::Add => {
                if let Stage::Ready(ready) = std::mem::replace(&mut self.stage, Stage::Loading) {
                    let (kind, pack, base, all_looks) = (self.kind, ready.pack.clone(), ready.base.clone(), ready.all_looks.clone());
                    self.stage = Stage::Installing(ready);
                    return (blocking(move || install(kind, &base, &pack, &all_looks), Message::Installed), Outcome::Nothing);
                }
            }
            Message::Installed(result) => {
                if let Stage::Installing(ready) = std::mem::replace(&mut self.stage, Stage::Loading) {
                    match result {
                        Ok(()) => {
                            paperloom_skin::refresh();
                            self.stage = Stage::Done(ready);
                        }
                        Err(why) => self.stage = Stage::Failed(format!("Couldn't add it: {why}")),
                    }
                }
            }
            Message::UseTheme => {
                if let (Kind::Theme, Stage::Done(ready)) = (self.kind, &self.stage) {
                    return (Task::none(), Outcome::UseTheme(ready.pack.id.clone()));
                }
            }
            Message::Close => return (Task::none(), Outcome::Close),
        }
        (Task::none(), Outcome::Nothing)
    }

    pub fn view<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let mut body = column![text("Add to Paperloom").font(theme::display_font()).size(22).color(t.foreground)].spacing(10);
        let close = || button(crate::ui::label("Close")).style(crate::ui::ghost).padding([8, 16]).on_press(Message::Close);
        match &self.stage {
            Stage::Loading => body = body.push(text(format!("Looking up “{}” in the Paperloom library…", self.id)).size(14).color(t.muted_foreground)),
            Stage::Failed(why) => {
                body = body.push(text(why.clone()).size(14).color(t.foreground));
                body = body.push(close());
            }
            Stage::Ready(ready) | Stage::Installing(ready) | Stage::Done(ready) => {
                if let Some(handle) = &ready.preview {
                    body = body.push(image(handle.clone()).width(Length::Fill).content_fit(iced::ContentFit::Contain));
                }
                body = body.push(text(self.kind.name()).size(12).color(t.muted_foreground));
                body = body.push(text(ready.pack.name.clone()).font(theme::body_font()).size(18).color(t.foreground));
                let by = if ready.pack.author.is_empty() { String::new() } else { format!("by {} · ", ready.pack.author) };
                body = body.push(text(format!("{by}{}", size(ready.pack.download_size()))).size(13).color(t.muted_foreground));
                if !ready.pack.description.is_empty() {
                    body = body.push(text(ready.pack.description.clone()).size(14).color(t.foreground));
                }
                let buttons: Element<'a, Message> = match &self.stage {
                    Stage::Ready(_) => row![button(crate::ui::label("Add")).style(crate::ui::primary).padding([8, 18]).on_press(Message::Add), close()].spacing(8).into(),
                    Stage::Installing(_) => text("Adding…").size(14).color(t.muted_foreground).into(),
                    _ => {
                        let found = match self.kind {
                            Kind::Look => "Added. Find it under Widgets → Looks.",
                            Kind::Stack => "Added. Find it under Widgets → Stacks.",
                            Kind::Theme => "Added to your themes.",
                        };
                        let mut done = column![text(found).size(14).color(t.foreground)].spacing(8);
                        let mut actions = row![].spacing(8).align_y(Alignment::Center);
                        if self.kind == Kind::Theme {
                            actions = actions.push(button(crate::ui::label("Use this theme")).style(crate::ui::primary).padding([8, 18]).on_press(Message::UseTheme));
                        }
                        done = done.push(actions.push(close()));
                        done.into()
                    }
                };
                body = body.push(buttons);
                body = body.push(text("From the Paperloom library. Its files are checked before they are added.").size(12).color(t.muted_foreground));
            }
        }
        crate::ui::modal(body, 460.0, Message::Close, t)
    }
}

fn size(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else {
        format!("{} KB", (bytes / 1024).max(1))
    }
}

fn find(kind: Kind, id: &str) -> Result<Box<Found>, String> {
    let (index, base) = match kind {
        Kind::Theme => paperloom_fetch::themes::fetch_index()?,
        _ => looks::fetch_index()?,
    };
    let list = if kind == Kind::Stack { &index.stacks } else { &index.packs };
    let pack = list.iter().find(|pack| pack.id == id).cloned().ok_or_else(|| format!("“{id}” is not in the Paperloom library."))?;
    let preview = match kind {
        Kind::Look => looks::fetch_preview(base, &pack),
        Kind::Stack => looks::fetch_stack_preview(base, &pack),
        Kind::Theme => paperloom_fetch::themes::fetch_preview(base, &pack),
    }
    .ok();
    Ok(Box::new(Found { pack, base: base.to_string(), all_looks: index.packs.clone(), preview }))
}

fn install(kind: Kind, base: &str, pack: &Pack, all_looks: &[Pack]) -> Result<(), String> {
    match kind {
        Kind::Look => looks::install(base, pack, &paperloom_common::skins_dir()),
        Kind::Theme => paperloom_fetch::themes::install(base, pack, &paperloom_common::themes_dir()),
        Kind::Stack => {
            // The looks it uses come along (the ones not on this computer yet).
            for need in &pack.needs {
                if let (false, Some(look)) = (paperloom_skin::get(need).is_some(), all_looks.iter().find(|look| &look.id == need)) {
                    looks::install(base, look, &paperloom_common::skins_dir())?;
                }
            }
            looks::install_stack(base, pack, &paperloom_common::stacks_dir())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_name_one_library_item() {
        assert_eq!(parse("paperloom://install/theme/fjord"), Some((Kind::Theme, "fjord".into())));
        assert_eq!(parse("paperloom://install/look/clock%2Forbit"), Some((Kind::Look, "clock/orbit".into())));
        assert_eq!(parse("paperloom://install/look/clock/orbit/"), Some((Kind::Look, "clock/orbit".into())));
        assert_eq!(parse("paperloom://install/stack/big-day"), Some((Kind::Stack, "big-day".into())));
    }

    #[test]
    fn anything_odd_is_refused() {
        for bad in ["paperloom://install/theme/", "paperloom://install/wallpaper/x", "paperloom://open/theme/x", "http://install/theme/x", "paperloom://install/theme/../x", "paperloom://install/theme/a%00b", "paperloom://install/theme/a b"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }
}
