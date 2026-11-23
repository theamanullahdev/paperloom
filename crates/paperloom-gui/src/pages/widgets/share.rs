//! Share a widget or a stack as a code or a file, and add one somebody sent.
//! The format and its checks are in `share_format`; this is the pop-up.

use iced::widget::{button, column, row, text, text_input};
use iced::{Alignment, Element, Length, Task};
use paperloom_common::widgets::Widgets;

use super::share_format::{self as format, Shared, What, CODE_LIMIT};
use crate::theme;

#[derive(Debug, Clone)]
pub enum Message {
    Close,
    Copy,
    SaveFile,
    Text(String),
    Paste,
    Pasted(Option<String>),
    ChooseFile,
    Add,
}

enum Dialog {
    Send { shared: Shared, code: String, notice: Option<String> },
    Receive { text: String, error: Option<String>, shared: Option<Shared>, notice: Option<String> },
}

/// What the widgets page must do after a message.
pub enum Outcome {
    Nothing,
    /// A widget was placed: save and show it.
    Placed(String),
    /// Looks or a stack were added to this computer.
    Added(String),
}

#[derive(Default)]
pub struct State {
    dialog: Option<Dialog>,
}

impl State {
    /// Opens "Share" for one placed widget; `Err` is a sentence for the page's notice.
    pub fn send_widget(&mut self, placed: &paperloom_common::widgets::Placed) -> Result<(), String> {
        self.send(format::of_widget(placed)?)
    }

    /// Opens "Share" for a stack on the desktop.
    pub fn send_stack(&mut self, all: &Widgets, stack: u32) -> Result<(), String> {
        self.send(format::of_stack(all, stack)?)
    }

    fn send(&mut self, shared: Shared) -> Result<(), String> {
        let code = shared.encode()?;
        self.dialog = Some(Dialog::Send { shared, code, notice: None });
        Ok(())
    }

    /// Opens "Add a shared widget", with `text` already in it when there is some
    /// (a file that was double-clicked or dropped).
    pub fn receive(&mut self, text: &str) -> Task<Message> {
        self.dialog = Some(Dialog::Receive { text: String::new(), error: None, shared: None, notice: None });
        if text.trim().is_empty() {
            return iced::widget::operation::focus(RECEIVE_INPUT);
        }
        self.read(text);
        Task::none()
    }

    fn read(&mut self, text: &str) {
        if let Some(Dialog::Receive { text: shown, error, shared, .. }) = &mut self.dialog {
            *shown = if text.len() > 200 { format!("{}…", &text[..120.min(text.len())]) } else { text.to_string() };
            match Shared::decode(text) {
                Ok(read) => {
                    *shared = Some(read);
                    *error = None;
                }
                Err(why) => {
                    *shared = None;
                    *error = Some(why);
                }
            }
        }
    }

    pub fn update(&mut self, message: Message, all: &mut Widgets) -> (Task<Message>, Outcome) {
        match message {
            Message::Close => self.dialog = None,
            Message::Copy => {
                if let Some(Dialog::Send { code, notice, .. }) = &mut self.dialog {
                    *notice = Some("Copied. Paste it in a chat for your friend.".into());
                    paperloom_fetch::counts::feature("share_made");
                    return (iced::clipboard::write(code.clone()), Outcome::Nothing);
                }
            }
            Message::SaveFile => {
                if let Some(Dialog::Send { shared, code, notice }) = &mut self.dialog {
                    let safe: String = shared.name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
                    let name = if safe.trim().is_empty() { "widget".to_string() } else { safe };
                    if let Some(path) = rfd::FileDialog::new().set_title("Save widget file").set_file_name(format!("{name}.paperloomwidget")).add_filter("Paperloom widget", &["paperloomwidget"]).save_file() {
                        *notice = Some(match std::fs::write(&path, &*code) {
                            Ok(()) => {
                                paperloom_fetch::counts::feature("share_made");
                                format!("Saved {}", path.display())
                            }
                            Err(err) => format!("Couldn't save the file: {err}"),
                        });
                    }
                }
            }
            Message::Text(value) => self.read(&value),
            Message::Paste => return (iced::clipboard::read().map(Message::Pasted), Outcome::Nothing),
            Message::Pasted(value) => {
                if let Some(value) = value {
                    self.read(&value);
                }
            }
            Message::ChooseFile => {
                if let Some(path) = rfd::FileDialog::new().set_title("Open a widget file").add_filter("Paperloom widget", &["paperloomwidget", "txt"]).pick_file() {
                    match std::fs::read_to_string(&path) {
                        Ok(text) => self.read(&text),
                        Err(err) => {
                            if let Some(Dialog::Receive { error, .. }) = &mut self.dialog {
                                *error = Some(format!("Couldn't read that file: {err}"));
                            }
                        }
                    }
                }
            }
            Message::Add => return (Task::none(), self.add(all)),
        }
        (Task::none(), Outcome::Nothing)
    }

    fn add(&mut self, all: &mut Widgets) -> Outcome {
        let Some(Dialog::Receive { shared: Some(shared), error, notice, .. }) = &mut self.dialog else { return Outcome::Nothing };
        let shared = shared.clone();
        let done = format::install_looks(&shared, &paperloom_common::skins_dir()).and_then(|_| {
            paperloom_skin::refresh();
            match shared.what() {
                What::Widget => format::place(&shared, all).map(|_| format!("{} is on your desktop.", shared.name)),
                What::Stack => format::install_stack(&shared, &paperloom_common::stacks_dir()).map(|_| format!("“{}” is in Widgets → Stacks.", shared.name)),
                What::Look => Ok(format!("“{}” is in Widgets → Looks.", shared.name)),
            }
        });
        match done {
            Ok(line) => {
                paperloom_fetch::counts::feature("share_imported");
                let placed = shared.what() == What::Widget;
                self.dialog = None;
                if placed {
                    Outcome::Placed(line)
                } else {
                    Outcome::Added(line)
                }
            }
            Err(why) => {
                *error = Some(format!("Couldn't add it: {why}"));
                *notice = None;
                Outcome::Nothing
            }
        }
    }

    pub fn view<'a>(&'a self, t: theme::Tokens) -> Option<Element<'a, Message>> {
        let dialog = self.dialog.as_ref()?;
        let muted = |line: String| text(line).size(13).color(t.muted_foreground);
        let body: Element<'a, Message> = match dialog {
            Dialog::Send { shared, code, notice } => {
                let what = match shared.what() {
                    What::Stack => format!("a stack of {} widgets", shared.widgets.len()),
                    What::Look => "a look".to_string(),
                    What::Widget => "a widget".to_string(),
                };
                let long = code.len() > CODE_LIMIT;
                let mut body = column![
                    text(format!("Share {}", shared.name)).font(theme::display_font()).size(22).color(t.foreground),
                    muted(format!("Anyone with Paperloom can add {what} from this. It carries its look, size and settings, and nothing from your computer.")),
                ]
                .spacing(10);
                if long {
                    body = body.push(muted("It has pictures or fonts in it, which is a lot for a chat message. Save it as a file and send that.".into()));
                }
                let copy = button(crate::ui::label("Copy code").center()).style(crate::ui::primary).padding([10, 16]).width(Length::Fill).on_press_maybe((!long).then_some(Message::Copy));
                let save = button(crate::ui::label("Save as a file…").center()).style(if long { crate::ui::primary } else { crate::ui::secondary }).padding([10, 16]).width(Length::Fill).on_press(Message::SaveFile);
                body = body.push(copy).push(save);
                if let Some(notice) = notice {
                    body = body.push(text(notice.clone()).size(13).color(t.primary));
                }
                body.push(button(crate::ui::label("Close")).style(crate::ui::ghost).on_press(Message::Close)).into()
            }
            Dialog::Receive { text: shown, error, shared, notice } => {
                let mut body = column![
                    text("Add a shared widget").font(theme::display_font()).size(22).color(t.foreground),
                    muted("Paste a code a friend sent, or open the file. You see what is in it before anything is added.".into()),
                    row![
                        text_input("paperloom-widget:1:…", shown).id(RECEIVE_INPUT).on_input(Message::Text).padding(8).width(Length::Fill),
                        button(crate::ui::label("Paste")).style(crate::ui::secondary).padding([8, 14]).on_press(Message::Paste),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                    button(crate::ui::label("Open a file…")).style(crate::ui::secondary).padding([8, 14]).on_press(Message::ChooseFile),
                ]
                .spacing(10);
                if let Some(error) = error {
                    body = body.push(text(error.clone()).size(13).color(t.destructive));
                }
                if let Some(shared) = shared {
                    let kind = match shared.what() {
                        What::Stack => format!("Stack of {} widgets", shared.widgets.len()),
                        What::Look => "Widget look".to_string(),
                        What::Widget => shared.widgets.first().and_then(|member| paperloom_common::widgets::entry(&member.kind)).map_or("Widget".to_string(), |entry| format!("{} widget", entry.name)),
                    };
                    body = body.push(text(shared.name.clone()).size(18).color(t.foreground));
                    body = body.push(muted(kind));
                    if let Some(zone) = shared.widgets.first().and_then(|member| member.options.get("timezone")) {
                        body = body.push(muted(format!("Shows the time in {zone}")));
                    }
                    if !shared.looks.is_empty() {
                        body = body.push(muted(format!("Brings {} look{} with it.", shared.looks.len(), if shared.looks.len() == 1 { "" } else { "s" })));
                    }
                    let same = shared.looks_already_here();
                    if !same.is_empty() {
                        body = body.push(muted(format!("Replaces your copy of: {}.", same.join(", "))));
                    }
                    let label = match shared.what() {
                        What::Widget => "Place on my desktop",
                        What::Stack => "Add to my stacks",
                        What::Look => "Add to my looks",
                    };
                    body = body.push(button(crate::ui::label(label).center()).style(crate::ui::primary).padding([10, 16]).width(Length::Fill).on_press(Message::Add));
                }
                if let Some(notice) = notice {
                    body = body.push(text(notice.clone()).size(13).color(t.primary));
                }
                body.push(button(crate::ui::label("Close")).style(crate::ui::ghost).on_press(Message::Close)).into()
            }
        };
        Some(crate::ui::modal(body, 480.0, Message::Close, t))
    }
}

const RECEIVE_INPUT: &str = "widget-share-code";
