//! Windows desktop integration: a window parented *behind the desktop
//! icons*, with libmpv rendering into it via `wid` (mpv takes an `HWND`
//! there, just like an X11 window id).
//!
//! The technique is the one Wallpaper Engine and Lively use. Sending
//! `Progman` (the desktop's top window) the undocumented message `0x052C`
//! makes Explorer split the desktop into an icon layer (`SHELLDLL_DefView`)
//! and a separate `WorkerW` behind it. Where that `WorkerW` ends up depends
//! on the Windows version:
//!
//! - Before Windows 11 24H2: a *top-level* `WorkerW` right after the
//!   top-level window that hosts `SHELLDLL_DefView`. Our window becomes its
//!   child.
//! - Windows 11 24H2 and later ("raised desktop"): the `WorkerW` is a
//!   *child of Progman*, and `SHELLDLL_DefView` is too. Our window becomes
//!   a child of Progman, z-ordered between the icons (above) and that
//!   `WorkerW` (below), and must be layered to be composited at all
//!   (Progman is `WS_EX_NOREDIRECTIONBITMAP`).
//!
//! Explorer restarting destroys our child window along with its parent, so
//! the main loop notices the window has gone and builds a new one (and a new
//! mpv) — the wallpaper comes back by itself.
//!
//! **Never run on a real Windows machine** — this dev environment is Linux.
//! It's built (cross-compile check locally, real MSVC build in CI) against
//! the exact `windows-sys` API, and the window-tree logic follows what
//! Lively ships, but "compiles" isn't "shows video behind the icons" until
//! someone runs it.

use std::error::Error;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::time::{Duration, Instant};

use libmpv2::Mpv;
use paperloom_common::ipc::Listener;
use paperloom_common::Settings;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, GetStockObject, MonitorFromWindow, BLACK_BRUSH, HBRUSH, MONITORINFO, MONITOR_DEFAULTTONULL,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows_sys::Win32::System::StationsAndDesktops::{CloseDesktop, OpenInputDesktop, DESKTOP_SWITCHDESKTOP};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows_sys::Win32::UI::Shell::{
    SHQueryUserNotificationState, QUERY_USER_NOTIFICATION_STATE, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumWindows, FindWindowExW, FindWindowW, GetClassNameW,
    GetForegroundWindow, GetSystemMetrics, GetTopWindow, GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId,
    IsIconic, IsWindow, IsWindowVisible, PeekMessageW, RegisterClassExW, SendMessageTimeoutW, SetForegroundWindow,
    SetLayeredWindowAttributes, SetWindowPos, ShowWindow, TranslateMessage, GWL_EXSTYLE, GW_HWNDNEXT, HWND_BOTTOM,
    HWND_TOPMOST, LWA_ALPHA, MSG, PM_REMOVE, SMTO_NORMAL, SM_CMONITORS, SM_CXSCREEN, SM_CXVIRTUALSCREEN, SM_CYSCREEN,
    SM_CYVIRTUALSCREEN, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_SHOW, WNDCLASSEXW, WS_CHILD, WS_CLIPCHILDREN, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};

use crate::player::{self, Flow};

const CLASS_NAME: &str = "PaperloomWallpaper";
const PREVIEW_CLASS_NAME: &str = "PaperloomPreview";

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

pub fn run(initial_path: Option<&str>) -> Result<(), Box<dyn Error>> {
    player::validate_cli_path(initial_path)?;
    // Physical pixels everywhere, so the window really covers a scaled
    // (125%/150%) screen instead of a DPI-virtualized fraction of it.
    // SAFETY: process-wide setting, called before any window exists.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };

    // Binding first doubles as the single-instance check: a second engine
    // fails here (the pipe already exists) before it touches the desktop.
    let mut listener = Listener::bind()?;
    register_class(CLASS_NAME)?;

    let mut startup = player::startup_wallpaper(initial_path);
    loop {
        let window = create_wallpaper_window()?;
        let mpv = Mpv::with_initializer(|init| {
            init.set_property("wid", window as i64)?;
            Ok(())
        })?;
        player::configure_wallpaper(&mpv)?;
        if let Some(path) = startup.take() {
            player::play(&mpv, &path);
        }

        match run_until_window_lost(window, &mpv, &mut listener) {
            Flow::Quit => {
                // SAFETY: our own window, destroyed once.
                unsafe { DestroyWindow(window) };
                return Ok(());
            }
            // Explorer restarted and took our window with it: build a new
            // one and pick up where we left off.
            Flow::Continue => {
                drop(mpv);
                startup = player::startup_wallpaper(None);
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

/// The poll loop: Windows messages, IPC, and (every ~2s) the pause
/// triggers. Returns `Quit` on an IPC quit, `Continue` if the window was
/// destroyed out from under us.
fn run_until_window_lost(window: HWND, mpv: &Mpv, listener: &mut Listener) -> Flow {
    let mut is_paused = false;
    let mut last_check = Instant::now();
    loop {
        pump_messages();
        if player::poll_ipc(listener, mpv) == Flow::Quit {
            return Flow::Quit;
        }

        if last_check.elapsed() >= Duration::from_secs(2) {
            last_check = Instant::now();
            // SAFETY: IsWindow accepts any handle value.
            if unsafe { IsWindow(window) } == 0 {
                return Flow::Continue;
            }
            let settings = paperloom_common::load();
            player::periodic(&settings);
            let should_pause = should_pause(&settings, window);
            if should_pause != is_paused {
                is_paused = should_pause;
                player::set_paused(mpv, is_paused);
            }
        }

        std::thread::sleep(Duration::from_millis(200));
    }
}

fn pump_messages() {
    // SAFETY: standard message pump on this thread's queue.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn register_class(name: &str) -> Result<(), Box<dyn Error>> {
    let class_name = wide(name);
    // SAFETY: the class struct is fully initialized; `class_name` outlives
    // the call (RegisterClassExW copies the string).
    let atom = unsafe {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(window_proc),
            hInstance: GetModuleHandleW(std::ptr::null()),
            hbrBackground: GetStockObject(BLACK_BRUSH) as HBRUSH,
            lpszClassName: class_name.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassExW(&class)
    };
    if atom == 0 {
        return Err(format!("RegisterClassExW failed: {}", std::io::Error::last_os_error()).into());
    }
    Ok(())
}

/// Builds the wallpaper window inside the desktop's window tree (see the
/// module docs for the two layouts). It spans the whole virtual screen, so
/// a multi-monitor setup gets one video across every monitor — the same
/// thing the X11 backend's root-sized window does.
fn create_wallpaper_window() -> Result<HWND, Box<dyn Error>> {
    let class_name = wide(CLASS_NAME);
    // SAFETY: plain Win32 calls on handles we just looked up; every pointer
    // passed is either null or a live, NUL-terminated wide string.
    unsafe {
        let progman = FindWindowW(wide("Progman").as_ptr(), std::ptr::null());
        if progman.is_null() {
            return Err("no Progman window — is Explorer running?".into());
        }
        // Ask Explorer to create the WorkerW behind the icons. Lively sends
        // both parameter forms; older builds only react to one of them.
        let mut result = 0usize;
        SendMessageTimeoutW(progman, 0x052C, 0xD, 0x1, SMTO_NORMAL, 1000, &mut result);
        SendMessageTimeoutW(progman, 0x052C, 0, 0, SMTO_NORMAL, 1000, &mut result);

        let (width, height) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
        let raised_workerw = FindWindowExW(progman, std::ptr::null_mut(), wide("WorkerW").as_ptr(), std::ptr::null());

        let window = if !raised_workerw.is_null() {
            // 24H2+ layout: child of Progman, between the icons and WorkerW.
            let window = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_NOACTIVATE,
                class_name.as_ptr(),
                std::ptr::null(),
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN,
                0,
                0,
                width,
                height,
                progman,
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            );
            if window.is_null() {
                return Err(format!("CreateWindowExW failed: {}", std::io::Error::last_os_error()).into());
            }
            SetLayeredWindowAttributes(window, 0, 255, LWA_ALPHA);
            let icons = FindWindowExW(progman, std::ptr::null_mut(), wide("SHELLDLL_DefView").as_ptr(), std::ptr::null());
            if !icons.is_null() {
                // "Insert after" the icon view = directly below it.
                SetWindowPos(window, icons, 0, 0, width, height, SWP_NOACTIVATE | SWP_SHOWWINDOW);
            }
            SetWindowPos(raised_workerw, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOACTIVATE | 0x0001 | 0x0002); // NOSIZE | NOMOVE
            window
        } else {
            let workerw = find_legacy_workerw().ok_or("couldn't find the desktop's WorkerW window")?;
            let window = CreateWindowExW(
                WS_EX_NOACTIVATE,
                class_name.as_ptr(),
                std::ptr::null(),
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN,
                0,
                0,
                width,
                height,
                workerw,
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            );
            if window.is_null() {
                return Err(format!("CreateWindowExW failed: {}", std::io::Error::last_os_error()).into());
            }
            window
        };
        ShowWindow(window, SW_SHOW);
        Ok(window)
    }
}

/// Pre-24H2: the top-level `WorkerW` that comes right after the top-level
/// window hosting `SHELLDLL_DefView` (the icons).
fn find_legacy_workerw() -> Option<HWND> {
    unsafe extern "system" fn visit(window: HWND, found: LPARAM) -> windows_sys::core::BOOL {
        let icons = FindWindowExW(window, std::ptr::null_mut(), wide("SHELLDLL_DefView").as_ptr(), std::ptr::null());
        if !icons.is_null() {
            let workerw = FindWindowExW(std::ptr::null_mut(), window, wide("WorkerW").as_ptr(), std::ptr::null());
            *(found as *mut HWND) = workerw;
        }
        1
    }
    let mut found: HWND = std::ptr::null_mut();
    // SAFETY: `visit` only writes through the pointer we pass, which lives
    // for the whole EnumWindows call.
    unsafe { EnumWindows(Some(visit), &mut found as *mut HWND as LPARAM) };
    (!found.is_null()).then_some(found)
}

// --- Pause triggers -------------------------------------------------------

fn should_pause(settings: &Settings, window: HWND) -> bool {
    (settings.pause_on_fullscreen_app && fullscreen_app_running())
        || (settings.pause_on_window_covering_desktop && desktop_covered(window))
        || (settings.pause_on_screen_lock && screen_locked())
        || (settings.pause_on_low_battery && battery_low(settings.low_battery_percent))
}

/// The shell's own "don't disturb the user" signal: a fullscreen app, a
/// D3D fullscreen game, or presentation mode.
fn fullscreen_app_running() -> bool {
    let mut state: QUERY_USER_NOTIFICATION_STATE = 0;
    // SAFETY: writes one enum value through a valid pointer.
    let ok = unsafe { SHQueryUserNotificationState(&mut state) } >= 0;
    ok && matches!(state, QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE)
}

/// Same approximation as the X11 backend — "some visible window is at
/// least as big as the screen" — done per monitor: paused only once every
/// monitor has a visible (not minimized, not cloaked on another virtual
/// desktop) window covering its work area. Walks top-level windows from
/// the top of the z-order down to the desktop itself.
fn desktop_covered(our_window: HWND) -> bool {
    // SAFETY: read-only window queries; every handle comes from the
    // z-order walk and is checked by the APIs themselves.
    unsafe {
        let monitors = GetSystemMetrics(SM_CMONITORS).max(1) as usize;
        let mut covered = Vec::new();
        let ours = GetCurrentProcessId();
        let mut window = GetTopWindow(std::ptr::null_mut());
        while !window.is_null() && covered.len() < monitors {
            let class = class_name(window);
            if class == "Progman" || class == "WorkerW" {
                break; // reached the desktop
            }
            if is_ordinary_visible_window(window, ours) && window != our_window {
                let monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONULL);
                let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..std::mem::zeroed() };
                let mut rect: RECT = std::mem::zeroed();
                if !monitor.is_null()
                    && GetMonitorInfoW(monitor, &mut info) != 0
                    && GetWindowRect(window, &mut rect) != 0
                    && rect.left <= info.rcWork.left
                    && rect.top <= info.rcWork.top
                    && rect.right >= info.rcWork.right
                    && rect.bottom >= info.rcWork.bottom
                    && !covered.contains(&monitor)
                {
                    covered.push(monitor);
                }
            }
            window = GetWindow(window, GW_HWNDNEXT);
        }
        covered.len() >= monitors
    }
}

unsafe fn is_ordinary_visible_window(window: HWND, our_process: u32) -> bool {
    if IsWindowVisible(window) == 0 || IsIconic(window) != 0 {
        return false;
    }
    let ex_style = GetWindowLongPtrW(window, GWL_EXSTYLE) as u32;
    if ex_style & WS_EX_TOOLWINDOW != 0 {
        return false;
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(window, &mut pid);
    if pid == our_process {
        return false;
    }
    // Windows on another virtual desktop (and some UWP shells) are "cloaked":
    // visible as far as IsWindowVisible knows, but not on screen.
    let mut cloaked = 0u32;
    let hr = DwmGetWindowAttribute(window, DWMWA_CLOAKED as u32, (&mut cloaked as *mut u32).cast(), 4);
    !(hr >= 0 && cloaked != 0) && !matches!(class_name(window).as_str(), "Shell_TrayWnd" | "Shell_SecondaryTrayWnd")
}

unsafe fn class_name(window: HWND) -> String {
    let mut buf = [0u16; 128];
    let len = GetClassNameW(window, buf.as_mut_ptr(), buf.len() as i32);
    String::from_utf16_lossy(&buf[..len.max(0) as usize])
}

/// The lock screen (and UAC's secure desktop) switch the *input desktop*
/// away from ours; a normal process can't open it then.
fn screen_locked() -> bool {
    // SAFETY: the returned handle is closed straight away.
    unsafe {
        let desktop = OpenInputDesktop(0, 0, DESKTOP_SWITCHDESKTOP);
        if desktop.is_null() {
            return true;
        }
        CloseDesktop(desktop);
        false
    }
}

/// On battery (not plugged in) and at or below the threshold. A desktop
/// with no battery reports flag 128 / percent 255 and never pauses.
fn battery_low(threshold_percent: u8) -> bool {
    let mut status: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    // SAFETY: fills the struct we own.
    if unsafe { GetSystemPowerStatus(&mut status) } == 0 {
        return false;
    }
    status.ACLineStatus == 0
        && status.BatteryFlag & 128 == 0
        && status.BatteryLifePercent != 255
        && status.BatteryLifePercent <= threshold_percent
}

// --- Preview --------------------------------------------------------------

/// Full-screen preview (`--preview <file>`): a topmost popup on the primary
/// monitor. mpv's own child window gets the actual mouse/keyboard messages,
/// so closing is detected by polling key/button state instead: any key or
/// click while the preview is in front closes it, and so does switching
/// away from it.
pub fn preview(path: &str) -> Result<(), Box<dyn Error>> {
    player::validate_cli_path(Some(path))?;
    // SAFETY: process-wide setting, called before any window exists.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    register_class(PREVIEW_CLASS_NAME)?;

    let class_name = wide(PREVIEW_CLASS_NAME);
    let title = wide("Paperloom preview");
    // SAFETY: as in `create_wallpaper_window`.
    let window = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_POPUP | WS_VISIBLE,
            0,
            0,
            GetSystemMetrics(SM_CXSCREEN),
            GetSystemMetrics(SM_CYSCREEN),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        )
    };
    if window.is_null() {
        return Err(format!("CreateWindowExW failed: {}", std::io::Error::last_os_error()).into());
    }
    // SAFETY: our own fresh window.
    unsafe {
        SetWindowPos(window, HWND_TOPMOST, 0, 0, 0, 0, 0x0001 | 0x0002); // NOSIZE | NOMOVE
        SetForegroundWindow(window);
    }

    let mpv = Mpv::with_initializer(|init| {
        init.set_property("wid", window as i64)?;
        Ok(())
    })?;
    mpv.set_property("loop-file", "inf".to_string())?;
    mpv.set_property("osc", false)?;
    mpv.set_property("input-default-bindings", false)?;
    mpv.set_property("input-vo-keyboard", false)?;
    mpv.set_property("cursor-autohide", "always".to_string())?;
    mpv.set_property("mute", true)?;
    mpv.set_property("panscan", 1.0)?;
    mpv.command("loadfile", &[path])?;

    // Ignore whatever is held at launch (the double-click that opened us):
    // only a fresh press closes the preview.
    let mut was_down = any_input_down();
    let opened = Instant::now();
    loop {
        pump_messages();
        // SAFETY: handle checks / foreground query only.
        let (alive, foreground) = unsafe { (IsWindow(window) != 0, GetForegroundWindow()) };
        if !alive {
            return Ok(());
        }
        let down = any_input_down();
        let in_front = foreground == window || is_descendant(foreground, window);
        if (down && !was_down && in_front) || (!in_front && opened.elapsed() > Duration::from_secs(1)) {
            // SAFETY: our own window.
            unsafe { DestroyWindow(window) };
            return Ok(());
        }
        was_down = down;
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn is_descendant(mut window: HWND, ancestor: HWND) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetParent;
    while !window.is_null() {
        if window == ancestor {
            return true;
        }
        // SAFETY: walking parents of a valid (or null) handle.
        window = unsafe { GetParent(window) };
    }
    false
}

/// Any mouse button or key currently held (every virtual-key code).
fn any_input_down() -> bool {
    // SAFETY: stateless key-state queries.
    (0x01..=0xFE).any(|vk| unsafe { GetAsyncKeyState(vk) } as u16 & 0x8000 != 0)
}
