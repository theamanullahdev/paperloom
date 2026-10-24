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
//! Startup is forgiving: Explorer may not be up yet at login, libmpv may
//! fail to start on an odd GPU, the screen layout may change. Instead of
//! exiting, the engine keeps retrying (with a plainer video setup after
//! the first failures), keeps answering IPC, and records the reason in
//! `engine.log` for the app to show.

use std::error::Error;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::time::{Duration, Instant};

use libmpv2::Mpv;
use paperloom_common::ipc::{Command, Listener};
use paperloom_common::Settings;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetParent, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN};
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
    let awareness = adopt_desktop_dpi();
    paperloom_common::log::info(&format!("DPI awareness: {awareness}"));

    // Binding first doubles as the single-instance check: a second engine
    // fails here (the pipe already exists) before it touches the desktop.
    let mut listener = Listener::bind()?;
    register_class(CLASS_NAME)?;

    let mut startup = player::startup_wallpaper(initial_path);
    let mut failures = 0u32;
    loop {
        // After two failures, fall back to the most compatible video setup.
        let (window, mpv) = match build(failures >= 2 || player::dev().safe_video) {
            Ok(pair) => {
                failures = 0;
                player::clear_failure();
                pair
            }
            Err(err) => {
                failures += 1;
                eprintln!("paperloom-engine: {err}");
                player::record_failure(&err);
                if wait_for_retry(&mut listener, &mut startup, Duration::from_secs(u64::from(failures.min(5)) * 2)) == Flow::Quit {
                    return Ok(());
                }
                continue;
            }
        };
        if let Some(path) = startup.take() {
            player::play(&mpv, &path);
        }

        match run_until_window_lost(window, &mpv, &mut listener) {
            Flow::Quit => {
                crate::tray_windows::shutdown();
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

/// The wallpaper window plus an mpv drawing into it. `safe` picks the most
/// compatible video output (Direct3D 11, software decoding) for machines
/// where the default fails.
fn build(safe: bool) -> Result<(HWND, Mpv), Box<dyn Error>> {
    let window = create_wallpaper_window()?;
    let made = (|| -> Result<Mpv, Box<dyn Error>> {
        let mpv = Mpv::with_initializer(|init| {
            init.set_property("wid", window as i64)?;
            init.set_property("log-file", paperloom_common::logs_dir().join("mpv.log").to_string_lossy().to_string())?;
            init.set_property("msg-level", player::mpv_log_level())?;
            if safe {
                init.set_property("vo", "gpu")?;
                init.set_property("gpu-api", "d3d11")?;
            }
            Ok(())
        })?;
        player::configure_wallpaper(&mpv)?;
        if safe {
            mpv.set_property("hwdec", "no".to_string())?;
        }
        Ok(mpv)
    })();
    match made {
        Ok(mpv) => Ok((window, mpv)),
        Err(err) => {
            // SAFETY: our own window, destroyed once.
            unsafe { DestroyWindow(window) };
            Err(format!("couldn't start the video player (libmpv): {err}").into())
        }
    }
}

/// Waits `wait` before the next attempt while still answering the app: a
/// QUIT ends the engine, a PLAY is remembered for when setup succeeds.
fn wait_for_retry(listener: &mut Listener, startup: &mut Option<String>, wait: Duration) -> Flow {
    let until = Instant::now() + wait;
    while Instant::now() < until {
        pump_messages();
        match listener.accept_command() {
            Ok(Some(Command::Quit)) => return Flow::Quit,
            Ok(Some(Command::Play(path))) => *startup = Some(path.to_string_lossy().into_owned()),
            _ => {}
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Flow::Continue
}

/// The poll loop: Windows messages, IPC, and (every ~2s) the pause
/// triggers. Returns `Quit` on an IPC quit, `Continue` if the window was
/// destroyed out from under us.
fn run_until_window_lost(window: HWND, mpv: &Mpv, listener: &mut Listener) -> Flow {
    let mut is_paused = false;
    let mut last_check = Instant::now();
    let mut area = wallpaper_area(paperloom_common::load().screen_target, true);
    loop {
        pump_messages();
        run_hotkeys(mpv);
        crate::tray_windows::run_actions(mpv);
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
            player::periodic(&settings, mpv);
            // A monitor was plugged in/out, the resolution changed, or the
            // "which screen" setting did: refit.
            // SAFETY: handle queries and one move of our own window.
            unsafe {
                let child = !GetParent(window).is_null();
                let wanted = wallpaper_area(settings.screen_target, child);
                if wanted != area && wanted.2 > 0 && wanted.3 > 0 {
                    area = wanted;
                    SetWindowPos(window, std::ptr::null_mut(), wanted.0, wanted.1, wanted.2, wanted.3, 0x0004 | SWP_NOACTIVATE);
                }
            }
            sync_hotkeys(&settings.hotkey_next, &settings.hotkey_pause);
            crate::tray_windows::sync(&settings);
            let should_pause = should_pause(&settings, window);
            if should_pause != is_paused {
                is_paused = should_pause;
                player::set_paused(mpv, is_paused);
            }
        }

        std::thread::sleep(Duration::from_millis(200));
    }
}

/// The monitors as (left, top, width, height, is_primary), in virtual-screen coordinates.
fn monitors() -> Vec<(i32, i32, i32, i32, bool)> {
    use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
    const MONITORINFOF_PRIMARY: u32 = 1;
    unsafe extern "system" fn visit(monitor: HMONITOR, _hdc: HDC, _rect: *mut RECT, list: LPARAM) -> windows_sys::core::BOOL {
        let found = &mut *(list as *mut Vec<(i32, i32, i32, i32, bool)>);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..std::mem::zeroed() };
        if GetMonitorInfoW(monitor, &mut info) != 0 {
            let r = info.rcMonitor;
            found.push((r.left, r.top, r.right - r.left, r.bottom - r.top, info.dwFlags & MONITORINFOF_PRIMARY != 0));
        }
        1
    }
    let mut found = Vec::new();
    // SAFETY: the callback only pushes into `found`, which outlives the call.
    unsafe { EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(visit), &mut found as *mut _ as LPARAM) };
    found
}

/// Where the wallpaper window should sit: (x, y, width, height). `relative`:
/// coordinates inside the desktop window (whose origin is the top-left of
/// the whole virtual screen) rather than on the screen.
fn wallpaper_area(target: paperloom_common::ScreenTarget, relative: bool) -> (i32, i32, i32, i32) {
    // SAFETY: plain metric queries.
    let (vx, vy, vw, vh) = unsafe {
        (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN), GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN))
    };
    let list = monitors();
    let chosen = paperloom_common::pick_monitor(target, &list.iter().map(|m| (*m, m.4)).collect::<Vec<_>>());
    match chosen {
        Some((left, top, width, height, _)) => if relative { (left - vx, top - vy, width, height) } else { (left, top, width, height) },
        None => if relative { (0, 0, vw, vh) } else { (vx, vy, vw, vh) },
    }
}

fn screen_size() -> (i32, i32) {
    // SAFETY: plain metric queries.
    unsafe { (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN)) }
}

fn pump_messages() {
    // SAFETY: standard message pump on this thread's queue.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            // A registered global shortcut (see `sync_hotkeys`).
            if msg.message == 0x0312 {
                HOTKEY_PRESSES.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(msg.wParam as i32);
                continue;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Shortcut ids (1 = next, 2 = pause) pressed since the last look.
static HOTKEY_PRESSES: std::sync::Mutex<Vec<i32>> = std::sync::Mutex::new(Vec::new());
static HOTKEYS: std::sync::Mutex<(String, String)> = std::sync::Mutex::new((String::new(), String::new()));

/// (Re)registers the two global shortcuts when the settings changed.
fn sync_hotkeys(next: &str, pause: &str) {
    use paperloom_common::hotkey::Hotkey;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
    let mut current = HOTKEYS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if current.0 == next && current.1 == pause {
        return;
    }
    *current = (next.to_string(), pause.to_string());
    let mut status = [String::new(), String::new()];
    for (id, text) in [(1, next), (2, pause)] {
        // SAFETY: thread-level hotkeys (no window); ids are ours.
        unsafe {
            UnregisterHotKey(std::ptr::null_mut(), id);
            if let Some(hotkey) = Hotkey::parse(text) {
                const NOREPEAT: u32 = 0x4000;
                // 0 means another program already owns that combination.
                let ok = RegisterHotKey(std::ptr::null_mut(), id, hotkey.windows_modifiers() | NOREPEAT, hotkey.windows_vk()) != 0;
                status[(id - 1) as usize] = if ok { "ok" } else { "in use" }.to_string();
            }
        }
    }
    player::set_hotkey_status(&status[0], &status[1]);
}

fn run_hotkeys(mpv: &Mpv) {
    let pressed = std::mem::take(&mut *HOTKEY_PRESSES.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
    for id in pressed {
        player::shortcut(mpv, if id == 1 { player::Shortcut::Next } else { player::Shortcut::TogglePause });
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

/// Takes on the same DPI awareness as the desktop (Explorer's `Progman`)
/// before any window exists. Windows refuses to parent a window of one
/// awareness inside a window of another, which shows up as a failing
/// `CreateWindowExW` ("handle is invalid") — so mimic the parent instead of
/// guessing. Waits a while for Explorer at login. Returns what was chosen.
fn adopt_desktop_dpi() -> String {
    use windows_sys::Win32::UI::HiDpi::{
        AreDpiAwarenessContextsEqual, GetWindowDpiAwarenessContext, DPI_AWARENESS_CONTEXT_SYSTEM_AWARE, DPI_AWARENESS_CONTEXT_UNAWARE,
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE,
    };
    // SAFETY: window lookups and a process-wide setting made before any window exists.
    unsafe {
        let mut progman = FindWindowW(wide("Progman").as_ptr(), std::ptr::null());
        for _ in 0..60 {
            if !progman.is_null() {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
            progman = FindWindowW(wide("Progman").as_ptr(), std::ptr::null());
        }
        let (name, context) = if progman.is_null() {
            ("per-monitor v2 (no desktop window found yet)", DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
        } else {
            let theirs = GetWindowDpiAwarenessContext(progman);
            let is = |wanted| AreDpiAwarenessContextsEqual(theirs, wanted) != 0;
            if is(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) {
                ("per-monitor v2 (same as the desktop)", DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
            } else if is(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE) {
                ("per-monitor (same as the desktop)", DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE)
            } else if is(DPI_AWARENESS_CONTEXT_SYSTEM_AWARE) {
                ("system (same as the desktop)", DPI_AWARENESS_CONTEXT_SYSTEM_AWARE)
            } else if is(DPI_AWARENESS_CONTEXT_UNAWARE) {
                ("unaware (same as the desktop)", DPI_AWARENESS_CONTEXT_UNAWARE)
            } else {
                ("per-monitor v2 (desktop's awareness not recognised)", DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
            }
        };
        SetProcessDpiAwarenessContext(context);
        name.to_string()
    }
}

/// Builds the wallpaper window inside the desktop's window tree (see the
/// module docs for the two layouts). It spans the whole virtual screen, so
/// a multi-monitor setup gets one video across every monitor — the same
/// thing the X11 backend's root-sized window does.
///
/// Several ways of getting a window into another process's window tree are
/// tried in turn (a child created directly, a pop-up re-parented, then the
/// pop-up covering the desktop); the one that worked is logged, and every
/// failure is kept so the self-test report can show them.
fn create_wallpaper_window() -> Result<HWND, Box<dyn Error>> {
    let mut failures: Vec<String> = Vec::new();
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

        // Explorer builds the layers asynchronously (and is slow just after
        // login): give it a moment before deciding which layout it chose.
        for _ in 0..20 {
            let raised = FindWindowExW(progman, std::ptr::null_mut(), wide("WorkerW").as_ptr(), std::ptr::null());
            if !raised.is_null() || find_legacy_workerw().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(150));
        }

        let target = paperloom_common::load().screen_target;
        let (x, y, width, height) = wallpaper_area(target, true);
        let raised_workerw = FindWindowExW(progman, std::ptr::null_mut(), wide("WorkerW").as_ptr(), std::ptr::null());
        let raised = !raised_workerw.is_null();
        let parent = if raised { progman } else { find_legacy_workerw().unwrap_or(std::ptr::null_mut()) };
        let ex_style = if raised { WS_EX_LAYERED | WS_EX_NOACTIVATE } else { WS_EX_NOACTIVATE };

        if !parent.is_null() {
            for (label, popup_first) in [("child window", false), ("pop-up moved into the desktop", true)] {
                match make_window(parent, ex_style, (x, y, width, height), popup_first) {
                    Ok(window) => {
                        if raised {
                            SetLayeredWindowAttributes(window, 0, 255, LWA_ALPHA);
                            let icons = FindWindowExW(progman, std::ptr::null_mut(), wide("SHELLDLL_DefView").as_ptr(), std::ptr::null());
                            if !icons.is_null() {
                                // "Insert after" the icon view = directly below it.
                                SetWindowPos(window, icons, x, y, width, height, SWP_NOACTIVATE | SWP_SHOWWINDOW);
                            }
                            SetWindowPos(raised_workerw, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOACTIVATE | 0x0001 | 0x0002); // NOSIZE | NOMOVE
                        }
                        ShowWindow(window, SW_SHOW);
                        paperloom_common::log::info(&format!("wallpaper window attached ({label}, {} layout)", if raised { "raised" } else { "classic" }));
                        return Ok(window);
                    }
                    Err(err) => failures.push(format!("{label}: {err}")),
                }
            }
        } else {
            failures.push("no desktop layer (WorkerW) to attach to".into());
        }

        // Last resort: a plain window over the desktop but under every other
        // window. The desktop icons are hidden beneath it, but the video shows.
        match make_cover_window(progman, wallpaper_area(target, false)) {
            Ok(window) => {
                paperloom_common::log::warn(&format!("wallpaper window is covering the desktop (icons hidden); attach failed: {}", failures.join(" | ")));
                return Ok(window);
            }
            Err(err) => failures.push(format!("cover the desktop: {err}")),
        }
    }
    Err(format!("couldn't create the wallpaper window — {}", failures.join(" | ")).into())
}

/// One attempt at a window parented into the desktop: directly as a child,
/// or as a pop-up that is then moved in with `SetParent`.
unsafe fn make_window(parent: HWND, ex_style: u32, area: (i32, i32, i32, i32), popup_first: bool) -> Result<HWND, String> {
    let (x, y, width, height) = area;
    use windows_sys::Win32::Foundation::{GetLastError, SetLastError};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetParent, SetWindowLongPtrW, GWL_STYLE, SWP_FRAMECHANGED, SWP_NOZORDER};

    let class_name = wide(CLASS_NAME);
    let module = GetModuleHandleW(std::ptr::null());
    SetLastError(0);
    let window = if popup_first {
        CreateWindowExW(ex_style, class_name.as_ptr(), std::ptr::null(), WS_POPUP, x, y, width, height, std::ptr::null_mut(), std::ptr::null_mut(), module, std::ptr::null())
    } else {
        CreateWindowExW(ex_style, class_name.as_ptr(), std::ptr::null(), WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN, x, y, width, height, parent, std::ptr::null_mut(), module, std::ptr::null())
    };
    if window.is_null() {
        let code = GetLastError();
        return Err(format!("CreateWindowExW failed: {} (code {code})", std::io::Error::from_raw_os_error(code as i32)));
    }
    if popup_first {
        SetLastError(0);
        if SetParent(window, parent).is_null() {
            let code = GetLastError();
            DestroyWindow(window);
            return Err(format!("SetParent failed: {} (code {code})", std::io::Error::from_raw_os_error(code as i32)));
        }
        SetWindowLongPtrW(window, GWL_STYLE, (WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN) as isize);
        SetWindowPos(window, std::ptr::null_mut(), x, y, width, height, SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED | SWP_SHOWWINDOW);
    }
    Ok(window)
}

/// A top-level window just above the desktop (see the last resort above).
unsafe fn make_cover_window(progman: HWND, area: (i32, i32, i32, i32)) -> Result<HWND, String> {
    let (x, y, width, height) = area;
    use windows_sys::Win32::Foundation::{GetLastError, SetLastError};
    let class_name = wide(CLASS_NAME);
    SetLastError(0);
    let window = CreateWindowExW(
        WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
        class_name.as_ptr(),
        std::ptr::null(),
        WS_POPUP | WS_VISIBLE,
        x,
        y,
        width,
        height,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        GetModuleHandleW(std::ptr::null()),
        std::ptr::null(),
    );
    if window.is_null() {
        let code = GetLastError();
        return Err(format!("CreateWindowExW failed: {} (code {code})", std::io::Error::from_raw_os_error(code as i32)));
    }
    // Under every normal window, over the desktop: sink it, then sink the desktop below it.
    SetWindowPos(window, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOACTIVATE | 0x0001 | 0x0002);
    SetWindowPos(progman, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOACTIVATE | 0x0001 | 0x0002);
    ShowWindow(window, SW_SHOW);
    Ok(window)
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
        || (!settings.pause_apps.is_empty() && foreground_app_matches(&settings.pause_apps))
        || (settings.pause_when_idle_minutes > 0 && idle_milliseconds() >= u64::from(settings.pause_when_idle_minutes) * 60_000)
}

/// Lower-case file name of the program with this process id ("" if unknown).
fn process_file_name(pid: u32) -> String {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: read-only query; the process handle is closed right away.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return String::new();
        }
        let mut buffer = [0u16; 520];
        let mut length = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) != 0;
        CloseHandle(process);
        if !ok {
            return String::new();
        }
        String::from_utf16_lossy(&buffer[..length as usize]).rsplit(['\\', '/']).next().unwrap_or_default().to_lowercase()
    }
}

/// The focused window's program (file name) or window class contains one of `apps`.
fn foreground_app_matches(apps: &[String]) -> bool {
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows_sys::Win32::Foundation::CloseHandle;
    // SAFETY: read-only queries; the process handle is closed right away.
    let (program, class) = unsafe {
        let window = GetForegroundWindow();
        if window.is_null() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(window, &mut pid);
        let mut program = String::new();
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if !process.is_null() {
            let mut buffer = [0u16; 520];
            let mut length = buffer.len() as u32;
            if QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) != 0 {
                program = String::from_utf16_lossy(&buffer[..length as usize]);
            }
            CloseHandle(process);
        }
        (program.rsplit(['\\', '/']).next().unwrap_or_default().to_lowercase(), class_name(window).to_lowercase())
    };
    apps.iter().map(|app| app.trim().to_lowercase()).any(|app| !app.is_empty() && (program.contains(&app) || class.contains(&app)))
}

/// Milliseconds since the last keyboard or mouse input.
fn idle_milliseconds() -> u64 {
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    // SAFETY: fills the struct we own.
    if unsafe { GetLastInputInfo(&mut info) } == 0 {
        return 0;
    }
    u64::from(unsafe { GetTickCount() }.wrapping_sub(info.dwTime))
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
    // Neither the engine nor Paperloom's own window (open while you pick a
    // wallpaper) counts as "something covering the desktop".
    if pid == our_process || process_file_name(pid).starts_with("paperloom") {
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
    let _ = mpv.set_property("osc", false); // absent when mpv is built without Lua (the Flatpak)
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

// --- Self-test --------------------------------------------------------------

/// Everything that can go wrong when attaching behind the desktop icons,
/// checked one at a time and printed (see `selftest.rs`).
pub fn selftest(clip: Option<&str>) {
    use crate::selftest::{fail, info, pass, text};

    info(format!("DPI awareness: {}", adopt_desktop_dpi()));
    match register_class(CLASS_NAME) {
        Ok(()) => pass("window class registered"),
        Err(err) => {
            fail(format!("window class: {err}"));
            return;
        }
    }

    info(format!(
        "screen: {} monitor(s), virtual desktop {}x{}",
        // SAFETY: plain metric queries.
        unsafe { GetSystemMetrics(SM_CMONITORS) },
        screen_size().0,
        screen_size().1
    ));
    info(format!(
        "state: fullscreen app {}, screen locked {}, idle {} s, battery low {}",
        fullscreen_app_running(),
        screen_locked(),
        idle_milliseconds() / 1000,
        battery_low(20)
    ));

    dump_desktop_tree();

    for (label, safe) in [("default video setup", false), ("safe video setup (Direct3D 11, software decoding)", true)] {
        match build(safe) {
            Ok((window, mpv)) => {
                pass(format!("{label}: window attached behind the icons and libmpv started"));
                if let Some(path) = clip {
                    window_playback(&mpv, window, path, label);
                }
                drop(mpv);
                // SAFETY: our own window, destroyed once.
                unsafe { DestroyWindow(window) };
            }
            Err(err) => fail(format!("{label}: {err}")),
        }
    }

    // What mpv itself complained about while doing the above.
    let tail = paperloom_common::log::tail("mpv", 25);
    if !tail.is_empty() {
        info("libmpv log (last lines):");
        for line in tail.lines() {
            info(format!("  {line}"));
        }
    }

    fn window_playback(mpv: &Mpv, window: HWND, path: &str, label: &str) {
        if let Err(err) = mpv.command("loadfile", &[path]) {
            fail(format!("{label}: loadfile failed: {err}"));
            return;
        }
        let started = Instant::now();
        let mut position = 0.0;
        while started.elapsed() < Duration::from_secs(6) {
            pump_messages();
            std::thread::sleep(Duration::from_millis(250));
            position = mpv.get_property::<f64>("time-pos").unwrap_or(0.0);
            if position > 1.5 {
                break;
            }
        }
        let configured = mpv.get_property::<bool>("vo-configured").unwrap_or(false);
        // SAFETY: handle check only.
        let alive = unsafe { IsWindow(window) } != 0;
        if position > 0.5 && configured && alive {
            pass(format!("{label}: video is playing in the wallpaper window ({position:.1}s)"));
        } else {
            fail(format!("{label}: no video after 6 s (time {position:.1}s, output ready {configured}, window alive {alive})"));
        }
        info(format!(
            "{label}: output {}, decoder {}, codec {}, size {}x{}, dropped {}",
            text(mpv, "current-vo"),
            text(mpv, "hwdec-current"),
            text(mpv, "video-codec"),
            mpv.get_property::<i64>("width").unwrap_or(0),
            mpv.get_property::<i64>("height").unwrap_or(0),
            mpv.get_property::<i64>("frame-drop-count").unwrap_or(0),
        ));
    }
}

/// Prints the parts of the desktop's window tree that decide where a
/// wallpaper can go: Progman, its children, and every WorkerW.
fn dump_desktop_tree() {
    use crate::selftest::{fail, info, pass};
    use windows_sys::Win32::UI::WindowsAndMessaging::EnumChildWindows;

    unsafe extern "system" fn child(window: HWND, list: LPARAM) -> windows_sys::core::BOOL {
        let lines = &mut *(list as *mut Vec<String>);
        let mut rect: RECT = std::mem::zeroed();
        GetWindowRect(window, &mut rect);
        lines.push(format!("    child {} visible={} rect=({},{},{},{})", class_name(window), IsWindowVisible(window) != 0, rect.left, rect.top, rect.right, rect.bottom));
        1
    }
    unsafe extern "system" fn top(window: HWND, list: LPARAM) -> windows_sys::core::BOOL {
        let lines = &mut *(list as *mut Vec<String>);
        let class = class_name(window);
        let has_icons = !FindWindowExW(window, std::ptr::null_mut(), wide("SHELLDLL_DefView").as_ptr(), std::ptr::null()).is_null();
        if class == "WorkerW" || has_icons {
            lines.push(format!("  top-level {class} icons-inside={has_icons} visible={}", IsWindowVisible(window) != 0));
        }
        1
    }

    // SAFETY: read-only window queries; the callbacks only push into a Vec
    // that outlives the enumeration.
    unsafe {
        let progman = FindWindowW(wide("Progman").as_ptr(), std::ptr::null());
        if progman.is_null() {
            fail("desktop: no Progman window — Explorer is not running (or this is not a normal desktop session)");
            return;
        }
        pass("desktop: Progman found");
        let mut result = 0usize;
        SendMessageTimeoutW(progman, 0x052C, 0xD, 0x1, SMTO_NORMAL, 1000, &mut result);
        SendMessageTimeoutW(progman, 0x052C, 0, 0, SMTO_NORMAL, 1000, &mut result);
        std::thread::sleep(Duration::from_millis(500));

        let mut lines = Vec::new();
        EnumChildWindows(progman, Some(child), &mut lines as *mut Vec<String> as LPARAM);
        info("desktop tree after asking Explorer for a WorkerW:");
        info("  Progman");
        for line in &lines {
            info(line);
        }
        let mut tops = Vec::new();
        EnumWindows(Some(top), &mut tops as *mut Vec<String> as LPARAM);
        for line in &tops {
            info(line);
        }
        let raised = !FindWindowExW(progman, std::ptr::null_mut(), wide("WorkerW").as_ptr(), std::ptr::null()).is_null();
        let legacy = find_legacy_workerw().is_some();
        info(format!("layout: raised (Windows 11 24H2 style) {raised}, classic WorkerW {legacy}"));
        if !raised && !legacy {
            fail("desktop: Explorer did not create a WorkerW layer to draw behind the icons");
        }
    }
}
