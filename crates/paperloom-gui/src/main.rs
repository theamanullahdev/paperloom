//! Paperloom control/settings GUI.
//!
//! This process is the user-facing app (library browser, settings, search).
//! It talks to `paperloom-engine` (the always-on renderer) over IPC rather
//! than rendering wallpapers itself, so this window can be closed without
//! stopping playback — see `engine_client`.

// A GUI app: no console window on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod cli;
mod desktop_colors;
mod engine_client;
mod icons;
mod instance;
mod lock_reset;
mod optimizer;
mod pages;
mod search;
mod telemetry;
mod theme;
mod ui;
mod widget;

/// What the GUI knows that the shared diagnostics report doesn't: how it was
/// installed, the machine's CPU/memory, the library.
pub fn diagnostics_extra() -> Vec<(String, Vec<String>)> {
    let (kind, reason) = paperloom_fetch::update::InstallKind::detect_with_reason();
    let mut system = sysinfo::System::new();
    system.refresh_cpu_all();
    system.refresh_memory();
    let cpu = system.cpus().first().map(|cpu| cpu.brand().to_string()).unwrap_or_default();
    let library = paperloom_common::Library::load();
    vec![
        ("Install".into(), vec![format!("kind: {} ({reason})", kind.label())]),
        (
            "Machine".into(),
            vec![
                format!("cpu: {cpu} ({} logical cores)", std::thread::available_parallelism().map_or(0, |n| n.get())),
                format!("memory: {} MB used of {} MB", system.used_memory() / 1_048_576, system.total_memory() / 1_048_576),
            ],
        ),
        (
            "Library".into(),
            vec![
                format!("wallpapers: {}", library.entries.len()),
                format!("collections: {}", library.collections.len()),
                format!("folder: {}", paperloom_common::wallpapers_dir().display()),
            ],
        ),
    ]
}

pub fn main() -> iced::Result {
    paperloom_common::log::init("gui");
    let args: Vec<String> = std::env::args().collect();
    let value_of = |flag: &str| args.iter().position(|arg| arg == flag).and_then(|at| args.get(at + 1)).cloned();

    // `--widget [--at X Y]`: the tray's themed pop-up, not the app. Launching
    // it again while it's open closes it (clicking the tray icon twice).
    if args.iter().any(|arg| arg == "--widget") {
        if !instance::claim("widget", "CLOSE") {
            return Ok(());
        }
        let at = args.iter().position(|arg| arg == "--at").and_then(|at| Some((args.get(at + 1)?.parse().ok()?, args.get(at + 2)?.parse().ok()?)));
        return widget::run(at);
    }

    // `paperloom-gui next|pause|set <name>|…`: drive the engine, no window.
    if let Some(code) = cli::run(&args) {
        std::process::exit(code);
    }

    // `--selftest`: run every check without opening a window; the report is
    // saved as logs/selftest.txt (and printed where a console exists).
    if args.iter().any(|arg| arg == "--selftest") {
        let report = pages::developers::run_suite(&paperloom_common::load());
        let path = paperloom_common::logs_dir().join("selftest.txt");
        let _ = std::fs::create_dir_all(paperloom_common::logs_dir());
        let _ = std::fs::write(&path, &report);
        println!("{report}\n\nSaved to {}", path.display());
        return Ok(());
    }

    // `--dev`: turn developer mode on and open the Developers page.
    if args.iter().any(|arg| arg == "--dev") {
        let mut settings = paperloom_common::load();
        settings.developer_mode = true;
        let _ = paperloom_common::save(&settings);
    }

    // `--diagnostics`: print the support report and exit (no window).
    if args.iter().any(|arg| arg == "--diagnostics") {
        println!("{}", paperloom_common::diagnostics::gather(&diagnostics_extra()));
        return Ok(());
    }

    // One Paperloom window: if it's already open, bring it forward (and pass
    // along what this launch asked for) instead of opening a second one.
    let request = if args.iter().any(|arg| arg == "--dev") {
        "PAGE developers".to_string()
    } else if args.iter().any(|arg| arg == "--tray-settings") {
        "TRAY-SETTINGS".to_string()
    } else if let Some(file) = app::share_file_arg() {
        let full = std::fs::canonicalize(&file).unwrap_or_else(|_| file.clone().into());
        format!("IMPORT {}", full.display())
    } else if let Some(page) = value_of("--page") {
        format!("PAGE {page}")
    } else {
        "SHOW".to_string()
    };
    if !instance::claim("gui", &request) {
        return Ok(());
    }

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
