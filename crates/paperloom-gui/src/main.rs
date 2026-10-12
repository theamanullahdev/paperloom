//! Paperloom control/settings GUI.
//!
//! This process is the user-facing app (library browser, settings, search).
//! It talks to `paperloom-engine` (the always-on renderer) over IPC rather
//! than rendering wallpapers itself, so this window can be closed without
//! stopping playback — see `engine_client`.

mod app;
mod engine_client;
mod icons;
mod pages;
mod theme;

pub fn main() -> iced::Result {
    iced::application(app::App::new, app::App::update, app::App::view)
        .title("Paperloom")
        .theme(app::App::theme)
        .subscription(app::App::subscription)
        .default_font(theme::BODY_FONT)
        .font(include_bytes!("../assets/fonts/IBMPlexSans.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/BricolageGrotesque.ttf").as_slice())
        .run()
}
