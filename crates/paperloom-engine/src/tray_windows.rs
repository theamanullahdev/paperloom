//! The icon in the Windows notification area (next to the clock).
//!
//! It lives in the engine, so the icon means "a wallpaper is running". A
//! left click opens the Paperloom widget next to the icon (now playing,
//! pause, stop, shortcuts); a right click opens the standard Windows menu.
//! All of it runs on the engine's own thread: the icon's hidden window is
//! served by the same message pump as the wallpaper window, and what the
//! menu asks for is carried out from the engine's loop (`run_actions`).
//!
//! The icon is shown while `Settings::show_tray_icon` is on, follows the
//! tray style and size settings, and is put back by itself if Explorer
//! restarts (`TaskbarCreated`).

use std::sync::Mutex;

use libmpv2::Mpv;
use paperloom_common::ipc::{self, Command};
use paperloom_common::{EngineState, Settings, TraySize, TrayStyle};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows_sys::Win32::UI::Shell::{Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu, GetCursorPos, GetSystemMetrics, PostMessageW,
    RegisterClassExW, RegisterWindowMessageW, SetForegroundWindow, TrackPopupMenu, HICON, ICONINFO, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING, SM_CXSMICON,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_NULL, WNDCLASSEXW,
};

use crate::player;

const CALLBACK: u32 = WM_APP + 1;
const WM_LBUTTONUP: isize = 0x0202;
const WM_RBUTTONUP: isize = 0x0205;
const ICON_ID: u32 = 1;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Widget(i32, i32),
    PauseResume,
    Stop,
    Mute,
    OpenApp,
    OpenPage(&'static str),
}

struct Tray {
    window: HWND,
    icon: HICON,
    /// What the icon currently looks like / says, to redo it only on change.
    look: (TrayStyle, TraySize, bool),
    tip: String,
    added: bool,
}

// SAFETY: only ever used from the engine's single message-pump thread.
unsafe impl Send for Tray {}

static TRAY: Mutex<Option<Tray>> = Mutex::new(None);
static ACTIONS: Mutex<Vec<Action>> = Mutex::new(Vec::new());
static TASKBAR_CREATED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static READD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// Called on the engine's ~2 s tick: shows, hides or restyles the icon.
pub fn sync(settings: &Settings) {
    let mut slot = TRAY.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let show = settings.show_tray_icon && !player::dev().disable_tray;
    if !show {
        if let Some(tray) = slot.take() {
            remove(&tray);
        }
        return;
    }
    let light_taskbar = taskbar_is_light();
    let look = (settings.tray_style, settings.tray_size, light_taskbar);
    let tip = tooltip();
    let tray = slot.get_or_insert_with(create);
    if tray.window.is_null() {
        return; // couldn't make the hidden window; try again next tick
    }
    let redo = READD.swap(false, std::sync::atomic::Ordering::Relaxed);
    if !tray.added || redo || tray.look != look || tray.tip != tip {
        if tray.look != look || tray.icon.is_null() {
            if !tray.icon.is_null() {
                // SAFETY: the icon is ours, destroyed once before being replaced.
                unsafe { DestroyIcon(tray.icon) };
            }
            tray.icon = build_icon(look.0, look.1, look.2);
            tray.look = look;
        }
        tray.tip = tip;
        tray.added = notify(tray, if tray.added && !redo { NIM_MODIFY } else { NIM_ADD });
    }
}

fn create() -> Tray {
    let class = wide("PaperloomTray");
    // SAFETY: standard class registration and a never-shown top-level window
    // (a message-only window would miss the `TaskbarCreated` broadcast).
    unsafe {
        TASKBAR_CREATED.store(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()), std::sync::atomic::Ordering::Relaxed);
        let class_info = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(window_proc),
            hInstance: GetModuleHandleW(std::ptr::null()),
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassExW(&class_info);
        let window = CreateWindowExW(0, class.as_ptr(), std::ptr::null(), 0, 0, 0, 0, 0, std::ptr::null_mut(), std::ptr::null_mut(), GetModuleHandleW(std::ptr::null()), std::ptr::null());
        Tray { window, icon: std::ptr::null_mut(), look: (TrayStyle::Auto, TraySize::Medium, false), tip: String::new(), added: false }
    }
}

fn remove(tray: &Tray) {
    if tray.added {
        let data = notify_data(tray);
        // SAFETY: valid struct for our own icon.
        unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
    }
    if !tray.icon.is_null() {
        // SAFETY: the icon is ours.
        unsafe { DestroyIcon(tray.icon) };
    }
}

fn notify_data(tray: &Tray) -> NOTIFYICONDATAW {
    // SAFETY: an all-zero NOTIFYICONDATAW is a valid starting point.
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = tray.window;
    data.uID = ICON_ID;
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.uCallbackMessage = CALLBACK;
    data.hIcon = tray.icon;
    for (slot, unit) in data.szTip.iter_mut().zip(tray.tip.encode_utf16().take(127)) {
        *slot = unit;
    }
    data
}

fn notify(tray: &Tray, message: u32) -> bool {
    let data = notify_data(tray);
    // SAFETY: valid struct for our own icon.
    unsafe { Shell_NotifyIconW(message, &data) != 0 }
}

fn tooltip() -> String {
    let state = EngineState::load();
    match state.current_wallpaper.and_then(|path| path.file_stem().map(|stem| stem.to_string_lossy().into_owned())) {
        Some(name) if state.user_paused => format!("Paperloom: paused, {name}"),
        Some(name) => format!("Paperloom: {name}"),
        None => "Paperloom".to_string(),
    }
}

/// Whether the taskbar is light (Windows 10/11 setting), so `Auto` can pick a
/// dark glyph for it.
fn taskbar_is_light() -> bool {
    let mut value: u32 = 0;
    let mut size = 4u32;
    // SAFETY: reads one DWORD into a local.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize").as_ptr(),
            wide("SystemUsesLightTheme").as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut value as *mut u32).cast(),
            &mut size,
        )
    };
    status == 0 && value == 1
}

macro_rules! asset {
    ($art:literal, $fill:literal, $px:literal) => {
        include_bytes!(concat!("../assets/tray/", $art, "-", $fill, "-", $px, ".argb")).as_slice()
    };
}

macro_rules! sizes {
    ($art:literal, $fill:literal) => {
        [(22, asset!($art, $fill, "22")), (32, asset!($art, $fill, "32")), (48, asset!($art, $fill, "48")), (64, asset!($art, $fill, "64"))]
    };
}

/// The icon for these settings, at the nearest size the shell asks for.
fn build_icon(style: TrayStyle, size: TraySize, light_taskbar: bool) -> HICON {
    let style = match style {
        TrayStyle::Auto if light_taskbar => TrayStyle::Black,
        TrayStyle::Auto => TrayStyle::White,
        other => other,
    };
    let sets: [(u32, &[u8]); 4] = match (style, size) {
        (TrayStyle::Colored, TraySize::Small) => sizes!("colored", "s"),
        (TrayStyle::Colored, TraySize::Medium) => sizes!("colored", "m"),
        (TrayStyle::Colored, TraySize::Large) => sizes!("colored", "l"),
        (_, TraySize::Small) => sizes!("glyph", "s"),
        (_, TraySize::Medium) => sizes!("glyph", "m"),
        (_, TraySize::Large) => sizes!("glyph", "l"),
    };
    // SAFETY: a plain metric query.
    let wanted = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16) as u32;
    let file = sets.iter().find(|(px, _)| *px >= wanted).unwrap_or(&sets[3]).1;
    let (width, height) = (i32::from_be_bytes([file[0], file[1], file[2], file[3]]), i32::from_be_bytes([file[4], file[5], file[6], file[7]]));
    // The files hold A,R,G,B per pixel; Windows wants B,G,R,A.
    let mut bgra = Vec::with_capacity(file.len() - 8);
    for pixel in file[8..].as_chunks::<4>().0 {
        let black = style == TrayStyle::Black && pixel[0] > 0;
        bgra.extend_from_slice(&if black { [0, 0, 0, pixel[0]] } else { [pixel[3], pixel[2], pixel[1], pixel[0]] });
    }
    // SAFETY: the bitmaps are created from buffers that outlive the calls and
    // are deleted once the icon owns copies of them.
    unsafe {
        let color = CreateBitmap(width, height, 1, 32, bgra.as_ptr().cast());
        let mask = CreateBitmap(width, height, 1, 1, std::ptr::null());
        let info = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info);
        DeleteObject(color);
        DeleteObject(mask);
        icon
    }
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == CALLBACK {
        match lparam {
            WM_LBUTTONUP => {
                let mut at = POINT { x: 0, y: 0 };
                GetCursorPos(&mut at);
                ACTIONS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(Action::Widget(at.x, at.y));
            }
            WM_RBUTTONUP => {
                if let Some(action) = show_menu(window) {
                    ACTIONS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(action);
                }
            }
            _ => {}
        }
        return 0;
    }
    let created = TASKBAR_CREATED.load(std::sync::atomic::Ordering::Relaxed);
    if created != 0 && message == created {
        READD.store(true, std::sync::atomic::Ordering::Relaxed);
        return 0;
    }
    DefWindowProcW(window, message, wparam, lparam)
}

/// The standard Windows menu at the pointer; what was picked, if anything.
unsafe fn show_menu(window: HWND) -> Option<Action> {
    let state = EngineState::load();
    let playing = state.current_wallpaper.is_some();
    let menu = CreatePopupMenu();
    let item = |id: usize, flags: u32, label: &str| {
        AppendMenuW(menu, MF_STRING | flags, id, wide(label).as_ptr());
    };
    item(1, 0, "Open widget");
    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
    item(2, if playing { 0 } else { MF_GRAYED }, if state.user_paused { "Resume" } else { "Pause" });
    item(3, if playing { 0 } else { MF_GRAYED }, "Stop wallpaper");
    item(4, if paperloom_common::load().muted { MF_CHECKED } else { 0 }, "Mute audio");
    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
    item(5, 0, "Open Paperloom");
    item(6, 0, "Usage");
    item(7, 0, "Settings");

    let mut at = POINT { x: 0, y: 0 };
    GetCursorPos(&mut at);
    // The menu only closes when clicking elsewhere if its owner is in front.
    SetForegroundWindow(window);
    let picked = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, at.x, at.y, 0, window, std::ptr::null());
    PostMessageW(window, WM_NULL, 0, 0);
    DestroyMenu(menu);
    match picked {
        1 => Some(Action::Widget(at.x, at.y)),
        2 => Some(Action::PauseResume),
        3 => Some(Action::Stop),
        4 => Some(Action::Mute),
        5 => Some(Action::OpenApp),
        6 => Some(Action::OpenPage("usage")),
        7 => Some(Action::OpenPage("settings")),
        _ => None,
    }
}

/// Carries out what the icon's menu or clicks asked for.
pub fn run_actions(mpv: &Mpv) {
    let pending = std::mem::take(&mut *ACTIONS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
    for action in pending {
        match action {
            Action::Widget(x, y) => spawn_gui(&["--widget", "--at", &x.to_string(), &y.to_string()]),
            Action::PauseResume => player::set_user_pause(mpv, !player::user_paused()),
            Action::Stop => {
                let _ = ipc::send(&Command::Stop);
            }
            Action::Mute => {
                let mut settings = paperloom_common::load();
                settings.muted = !settings.muted;
                let _ = paperloom_common::save(&settings);
                let _ = ipc::send(&Command::Mute(settings.muted));
            }
            Action::OpenApp => spawn_gui(&[]),
            Action::OpenPage(page) => spawn_gui(&["--page", page]),
        }
    }
}

fn spawn_gui(args: &[&str]) {
    if let Err(err) = paperloom_common::process::command("paperloom-gui").args(args).spawn() {
        paperloom_common::log::error(&format!("couldn't start paperloom-gui {args:?}: {err}"));
    }
}

/// Takes the icon away when the engine exits.
pub fn shutdown() {
    if let Some(tray) = TRAY.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take() {
        remove(&tray);
    }
}
