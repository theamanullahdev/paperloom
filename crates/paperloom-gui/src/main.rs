//! Paperloom control/settings GUI.
//!
//! This process is the user-facing app (library browser, settings, search).
//! It talks to `paperloom-engine` (the always-on renderer) over IPC rather
//! than rendering wallpapers itself, so this window can be closed without
//! stopping playback — see `engine_client`.

// A GUI app: no console window on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod engine_client;
mod icons;
mod pages;
mod search;
mod theme;
mod ui;

pub fn main() -> iced::Result {
    iced::application(app::App::new, app::App::update, app::App::view)
        .title("Paperloom")
        .theme(app::App::theme)
        .subscription(app::App::subscription)
        .default_font(theme::BODY_FONT)
        .font(include_bytes!("../assets/fonts/IBMPlexSans.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/BricolageGrotesque.ttf").as_slice())
        .window(window_settings())
        .run()
}

/// The window's own icon (title bar, Alt+Tab, Windows taskbar), and — on
/// Linux — the application id the desktop matches against
/// `paperloom.desktop` (`StartupWMClass`), which is what makes the panel
/// show Paperloom's logo instead of a generic one.
fn window_settings() -> iced::window::Settings {
    let icon = iced::window::icon::from_file_data(include_bytes!("../assets/icon-256.png"), None).ok();
    #[allow(unused_mut)]
    let mut settings = iced::window::Settings { icon, min_size: Some(iced::Size::new(160.0, 160.0)), ..Default::default() };
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // Inside Flatpak the desktop file is named after the app id.
        settings.platform_specific.application_id = std::env::var("FLATPAK_ID").unwrap_or_else(|_| "paperloom".to_string());
    }
    settings
}
