//! "Forgot password?": gets back into the Hidden section without losing
//! anything. The password is only a gate (nothing is encrypted), so proving
//! who you are with the computer's own login is enough to choose a new one,
//! or to remove it.
//!
//! One pop-up for the whole app, opened from the Hidden section's unlock
//! screen and from Settings. Steps: explain → system login window → new
//! password.

use iced::widget::{button, column, row, text, text_input};
use iced::{Alignment, Element, Length, Task};
use paperloom_common::os_auth::{self, Failure};

use crate::pages::online::blocking;
use crate::theme;

const MIN_LENGTH: usize = 4;

#[derive(Debug, Clone)]
pub enum Message {
    Confirm,
    Checked(Result<(), Failure>),
    New(String),
    Again(String),
    Save,
    RemoveInstead,
    Close,
}

enum Stage {
    Intro { note: Option<String> },
    Waiting,
    Unavailable(String),
    NewPassword { new: String, again: String, error: Option<String> },
}

pub struct State {
    stage: Stage,
}

/// What the app should do after a message.
pub enum Outcome {
    Nothing,
    Close,
    /// The new stored password (`None` removes it). The person has been confirmed.
    Replace(Option<String>),
}

impl State {
    pub fn new() -> Self {
        State { stage: Stage::Intro { note: None } }
    }

    /// The pop-up can't be dismissed while the system's own window is open.
    pub fn can_close(&self) -> bool {
        !matches!(self.stage, Stage::Waiting)
    }

    pub fn update(&mut self, message: Message) -> (Task<Message>, Outcome) {
        // An answer only counts while we are actually waiting for one.
        if matches!(message, Message::Checked(_)) && !matches!(self.stage, Stage::Waiting) {
            return (Task::none(), Outcome::Nothing);
        }
        match message {
            Message::Confirm => {
                self.stage = Stage::Waiting;
                let task = Task::perform(blocking(|| os_auth::verify("Confirm it's you to reset the Paperloom password")), Message::Checked);
                return (task, Outcome::Nothing);
            }
            Message::Checked(Ok(())) => self.stage = Stage::NewPassword { new: String::new(), again: String::new(), error: None },
            Message::Checked(Err(Failure::Declined)) => self.stage = Stage::Intro { note: Some(Failure::Declined.to_string()) },
            Message::Checked(Err(Failure::Unavailable(why))) => self.stage = Stage::Unavailable(why),
            Message::New(text) => {
                if let Stage::NewPassword { new, error, .. } = &mut self.stage {
                    *new = text;
                    *error = None;
                }
            }
            Message::Again(text) => {
                if let Stage::NewPassword { again, error, .. } = &mut self.stage {
                    *again = text;
                    *error = None;
                }
            }
            Message::Save => {
                if let Stage::NewPassword { new, again, error } = &mut self.stage {
                    match check_new(new, again) {
                        Ok(()) => return (Task::none(), Outcome::Replace(Some(paperloom_common::lock::make(new)))),
                        Err(why) => *error = Some(why.to_string()),
                    }
                }
            }
            Message::RemoveInstead => {
                if matches!(self.stage, Stage::NewPassword { .. }) {
                    return (Task::none(), Outcome::Replace(None));
                }
            }
            Message::Close => {
                if self.can_close() {
                    return (Task::none(), Outcome::Close);
                }
            }
        }
        (Task::none(), Outcome::Nothing)
    }

    pub fn view<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let title = |label: &'static str| text(label).font(theme::DISPLAY_FONT).size(20).color(t.foreground);
        let body = |label: String| text(label).size(13).color(t.muted_foreground);
        let content: Element<'a, Message> = match &self.stage {
            Stage::Intro { note } => {
                let mut column = column![
                    title("Reset the password"),
                    body("Confirm it's you with your computer's own login. Your hidden wallpapers stay exactly where they are; only the password is replaced.".into()),
                ]
                .spacing(10);
                if let Some(note) = note {
                    column = column.push(text(note.clone()).size(13).color(t.destructive));
                }
                column
                    .push(
                        row![
                            button(crate::ui::label("Confirm with my computer's login")).style(crate::ui::primary).padding([8, 16]).on_press(Message::Confirm),
                            button(crate::ui::label("Cancel")).style(crate::ui::ghost).padding([8, 14]).on_press(Message::Close),
                        ]
                        .spacing(8)
                        .wrap()
                        .vertical_spacing(6),
                    )
                    .into()
            }
            Stage::Waiting => column![title("Waiting for your computer"), body("Use the login window that opened. If you can't see it, look behind this window.".into())].spacing(10).into(),
            Stage::Unavailable(why) => column![
                title("This computer can't confirm it from here"),
                body(why.clone()),
                body(format!("The manual way: close Paperloom, open {} in a text editor, delete the line that starts with hidden_lock, then start Paperloom again. The Hidden section then has no password.", paperloom_common::config_file().display())),
                button(crate::ui::label("Close")).style(crate::ui::secondary).padding([8, 14]).on_press(Message::Close),
            ]
            .spacing(10)
            .into(),
            Stage::NewPassword { new, again, error } => {
                let mut column = column![
                    title("Choose a new password"),
                    body("You're confirmed. Pick a new password, or remove it so the Hidden section opens without one.".into()),
                    text_input("New password", new).secure(true).on_input(Message::New).padding(8).width(Length::Fill),
                    text_input("Type it again", again).secure(true).on_input(Message::Again).on_submit(Message::Save).padding(8).width(Length::Fill),
                ]
                .spacing(10);
                if let Some(error) = error {
                    column = column.push(text(error.clone()).size(13).color(t.destructive));
                }
                column
                    .push(
                        row![
                            button(crate::ui::label("Set new password")).style(crate::ui::primary).padding([8, 16]).on_press(Message::Save),
                            button(crate::ui::label("Remove the password")).style(crate::ui::secondary).padding([8, 14]).on_press(Message::RemoveInstead),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center)
                        .wrap()
                        .vertical_spacing(6),
                    )
                    .into()
            }
        };
        crate::ui::modal(content, 460.0, Message::Close, t)
    }
}

/// The rules for a new password, in plain words.
fn check_new(new: &str, again: &str) -> Result<(), &'static str> {
    if new.chars().count() < MIN_LENGTH {
        Err("Use at least 4 characters.")
    } else if new != again {
        Err("The two passwords are different.")
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn send(state: &mut State, message: Message) -> Outcome {
        state.update(message).1
    }

    #[test]
    fn new_password_rules() {
        assert_eq!(check_new("abc", "abc"), Err("Use at least 4 characters."));
        assert_eq!(check_new("abcd", "abce"), Err("The two passwords are different."));
        assert_eq!(check_new("abcd", "abcd"), Ok(()));
    }

    #[test]
    fn declined_goes_back_and_confirmed_moves_on() {
        let mut state = State::new();
        state.stage = Stage::Waiting;
        assert!(!state.can_close());
        let outcome = send(&mut state, Message::Checked(Err(Failure::Declined)));
        assert!(matches!(outcome, Outcome::Nothing));
        assert!(matches!(state.stage, Stage::Intro { note: Some(_) }));
        state.stage = Stage::Waiting;
        send(&mut state, Message::Checked(Ok(())));
        assert!(matches!(state.stage, Stage::NewPassword { .. }));
    }

    #[test]
    fn only_a_confirmed_person_can_replace_the_password() {
        let mut state = State::new();
        // Still at the first step: saving, removing, or a stray "confirmed" does nothing.
        assert!(matches!(send(&mut state, Message::Save), Outcome::Nothing));
        assert!(matches!(send(&mut state, Message::RemoveInstead), Outcome::Nothing));
        send(&mut state, Message::Checked(Ok(())));
        assert!(matches!(state.stage, Stage::Intro { .. }));
        // The system said yes while we were waiting for it.
        state.stage = Stage::Waiting;
        send(&mut state, Message::Checked(Ok(())));
        send(&mut state, Message::New("secret".into()));
        send(&mut state, Message::Again("secret".into()));
        match send(&mut state, Message::Save) {
            Outcome::Replace(Some(stored)) => assert!(paperloom_common::lock::verify(&stored, "secret")),
            _ => panic!("expected a new password"),
        }
        assert!(matches!(send(&mut state, Message::RemoveInstead), Outcome::Replace(None)));
    }

    #[test]
    fn unavailable_shows_the_manual_way() {
        let mut state = State::new();
        state.stage = Stage::Waiting;
        send(&mut state, Message::Checked(Err(Failure::Unavailable("no polkit".into()))));
        assert!(matches!(state.stage, Stage::Unavailable(_)));
        assert!(matches!(send(&mut state, Message::Close), Outcome::Close));
    }
}
