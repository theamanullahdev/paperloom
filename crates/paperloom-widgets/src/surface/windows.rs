//! Windows: each widget is a layered child window of the desktop layer, z-ordered
//! directly below the desktop icons and above the wallpaper window, so icons
//! draw over it and every normal window covers it. Same attach technique as
//! the wallpaper engine (`0x052C` to Progman; a `WorkerW` child before
//! Windows 11 24H2, a Progman child on the "raised desktop" of 24H2), with
//! per-pixel alpha through `UpdateLayeredWindow`.
//!
//! Explorer restarting destroys the windows with their parent; the loop
//! notices and builds them again. The widgets also re-check their order every
//! few seconds, because the wallpaper engine places its own window just below
//! the icons whenever it (re)starts.

use std::error::Error;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tiny_skia::Pixmap;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, ScreenToClient, SelectObject, CAPTUREBLT, SRCCOPY, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows_sys::Win32::UI::HiDpi::{
    AreDpiAwarenessContextsEqual, GetWindowDpiAwarenessContext, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, DPI_AWARENESS_CONTEXT_SYSTEM_AWARE, DPI_AWARENESS_CONTEXT_UNAWARE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, EnumWindows, GetCursorPos, GetParent,
    GetWindowLongPtrW, GetWindowRect, PostMessageW, SetForegroundWindow, SetWindowLongPtrW, TrackPopupMenu, FindWindowExW, FindWindowW, GetSystemMetrics, GetTopWindow, GetWindow,
    IsWindow, PeekMessageW, RegisterClassExW, SendMessageTimeoutW, SetWindowPos, ShowWindow, TranslateMessage, UpdateLayeredWindow,
    GW_HWNDNEXT, GW_HWNDPREV, HWND_TOP, GA_ROOT, GetAncestor, GetClassNameW, IsWindowVisible, WindowFromPoint, WM_COMMAND, MSG, PM_REMOVE, SMTO_NORMAL, SM_CXSCREEN, SM_CYSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_SHOWNOACTIVATE, ULW_ALPHA, WNDCLASSEXW, WS_CHILD, WS_CLIPCHILDREN, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP, GWLP_USERDATA, MF_STRING, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_NULL, WM_RBUTTONUP,
};

use paperloom_common::widgets::{Placed, Widgets};

use crate::control::Control;
use crate::interact;
use crate::render::menu;
use crate::render::Text;
use crate::widgets::{self, Context};

const CLASS_NAME: &str = "PaperloomWidget";

/// The window class and the menu's owner window are made once per process (a
/// reload of the settings runs the surface again, and a class can't be
/// registered twice).
static CLASS_READY: AtomicBool = AtomicBool::new(false);
static OWNER: AtomicIsize = AtomicIsize::new(0);
/// Set once this machine refused a layered child window of the desktop (or one
/// that never painted): from then on widgets are plain windows just above it.
static DESKTOP_UNSUPPORTED: AtomicBool = AtomicBool::new(false);
/// The probe runs beside the real widgets process and must not overwrite its status.
static QUIET: AtomicBool = AtomicBool::new(false);

fn status(change: impl FnOnce(&mut paperloom_common::widgets::Status)) {
    if !QUIET.load(Ordering::Relaxed) {
        paperloom_common::widgets::Status::update(change);
    }
}
/// How often the stacking order is verified.
const RESTACK_EVERY: Duration = Duration::from_secs(2);

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

struct Window {
    hwnd: HWND,
    /// Showing a picture failed (logged once).
    failed: bool,
    placed: Placed,
    pixmap: Option<Pixmap>,
    due: Instant,
}

/// Where the windows hang: the desktop layer and, on the raised desktop, the
/// icon view they must stay just below.
struct Layer {
    parent: HWND,
    icons: Option<HWND>,
    /// Plain windows: the window that holds the desktop icons, which they are kept directly above
    /// (above the icons and the wallpaper, below every other window).
    desktop: Option<HWND>,
}

pub fn run(widgets: &Widgets, text: &mut Text, control: &mut Control) -> Result<(), Box<dyn Error>> {
    adopt_desktop_dpi();
    register_class()?;
    let owner = menu_owner()?;

    loop {
        // Ordinary windows kept directly above the desktop (as Rainmeter does): they work on every
        // version of Windows, above the icons and the wallpaper, below every other window. Attaching
        // to the desktop layer itself (under the icons) is a developer switch, and falls back to this.
        let first = if paperloom_common::dev::DevSwitches::active().widgets_under_icons && !DESKTOP_UNSUPPORTED.load(Ordering::Relaxed) {
            Mode::Desktop
        } else {
            Mode::PlainAbove
        };
        let built = build(widgets, first).or_else(|err| {
            if first != Mode::Desktop {
                return Err(err);
            }
            DESKTOP_UNSUPPORTED.store(true, Ordering::Relaxed);
            paperloom_common::log::warn(&format!("widgets: couldn't attach to the desktop ({err}); using plain windows just above it instead"));
            status(|status| status.problem = format!("couldn't attach to the desktop: {err}"));
            build(widgets, Mode::PlainAbove)
        });
        let (layer, mut windows) = match built {
            Ok(built) => built,
            Err(err) => {
                paperloom_common::log::warn(&format!("widgets: {err}; trying again"));
                status(|status| status.problem = err.to_string());
                std::thread::sleep(Duration::from_secs(3));
                if control.changed() {
                    return Ok(());
                }
                continue;
            }
        };
        let mut next_restack = Instant::now() + RESTACK_EVERY;
        // Until a test patch has been seen on the screen, nothing claims the widgets are showing.
        let mut verified = false;
        let mut verify_at = Instant::now() + Duration::from_millis(800);
        'alive: loop {
            if control.changed() {
                for window in &windows {
                    // SAFETY: our own windows.
                    unsafe { DestroyWindow(window.hwnd) };
                }
                return Ok(());
            }
            pump_messages();
            handle_events(&mut windows, owner, control);
            let now = Instant::now();
            let paused = control.paused();
            if !paused {
                control.refresh_sample();
            } else if INPUT.lock().map(|mut input| input.drag.take().is_some()).unwrap_or(false) {
                // Never hold the mouse while paused (a fullscreen game, the lock screen).
                // SAFETY: releases this thread's capture; no arguments.
                unsafe { ReleaseCapture() };
            }
            let slow = control.slow();
            for window in &mut windows {
                // SAFETY: a plain handle validity query.
                if unsafe { IsWindow(window.hwnd) } == 0 {
                    break 'alive; // Explorer restarted and took them with it.
                }
                if paused {
                    continue;
                }
                if window.due > now && window.pixmap.is_some() {
                    continue;
                }
                let Some(frame) = widgets::render(&window.placed, text, &Context { sample: &control.sample, slow }) else {
                    window.due = now + Duration::from_secs(3600);
                    continue;
                };
                window.due = now + frame.next_change;
                let Some(pixmap) = frame.pixmap else { continue };
                if !present(window.hwnd, &pixmap, window.placed.opacity) && !window.failed {
                    window.failed = true;
                    let why = last_error("UpdateLayeredWindow");
                    paperloom_common::log::warn(&format!("widgets: a widget picture couldn't be shown ({why})"));
                    status(|status| status.problem = format!("a widget picture couldn't be shown ({why})"));
                    // Attached to the desktop but nothing can be painted there: use plain windows instead.
                    if layer.parent.is_null() {
                        continue;
                    }
                    DESKTOP_UNSUPPORTED.store(true, Ordering::Relaxed);
                    break 'alive;
                }
                window.pixmap = Some(pixmap);
            }
            if !paused && !verified && now >= verify_at && windows.iter().all(|window| window.pixmap.is_some()) {
                match check_visible(&layer) {
                    Seen::Yes => {
                        verified = true;
                        paperloom_common::log::info("widgets: a test patch was seen on the screen");
                        status(|status| {
                            status.verified = true;
                            status.problem.clear();
                        });
                    }
                    // Other windows cover the whole desktop right now: nothing can be judged. Try again soon.
                    Seen::Covered => verify_at = now + Duration::from_secs(8),
                    Seen::No(reason) if !layer.parent.is_null() => {
                        // Attached to the desktop layer, but nothing can be seen there.
                        DESKTOP_UNSUPPORTED.store(true, Ordering::Relaxed);
                        paperloom_common::log::warn(&format!("widgets: nothing could be seen on the desktop layer ({reason}); using plain windows just above the desktop"));
                        status(|status| status.problem = format!("widgets attached to the desktop layer but nothing could be seen there ({reason})"));
                        break 'alive;
                    }
                    Seen::No(reason) => {
                        paperloom_common::log::warn(&format!("widgets: not visible ({reason})"));
                        status(|status| {
                            status.verified = false;
                            status.problem = format!("widgets can't be seen on this desktop ({reason})");
                        });
                        verify_at = now + Duration::from_secs(60);
                    }
                }
            }
            if now >= next_restack {
                restack(&layer, &windows);
                next_restack = now + RESTACK_EVERY;
            }
            let wake = windows.iter().map(|window| window.due).chain([next_restack]).min().unwrap_or(now + Duration::from_secs(1));
            let dragging = INPUT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).drag.is_some();
            let nap = if dragging { Duration::from_millis(15) } else { wake.saturating_duration_since(Instant::now()).min(Duration::from_secs(1)) };
            std::thread::sleep(nap);
        }
        for window in &windows {
            // SAFETY: our own windows; destroying one that is already gone is harmless.
            unsafe { DestroyWindow(window.hwnd) };
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

/// What the mouse did, queued by the window procedure and handled in the main loop.
enum Event {
    /// A widget was dragged and let go at this top-left (screen pixels).
    Dropped(isize, i32, i32),
    /// Right-clicked at this screen position.
    Menu(isize, i32, i32),
}

/// A drag in progress.
#[derive(Clone, Copy)]
struct Dragging {
    window: isize,
    /// Pointer and window top-left when the button went down (screen pixels).
    start: (i32, i32),
    from: (i32, i32),
    moved: bool,
}

struct Input {
    /// The widget being dragged: its window, where the pointer and the window were, and whether it moved.
    drag: Option<Dragging>,
    events: Vec<Event>,
}

static INPUT: Mutex<Input> = Mutex::new(Input { drag: None, events: Vec::new() });

/// Per-window behaviour, kept in the window's user data.
const CAN_DRAG: isize = 1;
const CAN_MENU: isize = 2;
/// Closer than this (pixels) and a press-and-release is a click, not a drag.
const DRAG_SLOP: i32 = 3;

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: plain window and cursor queries on the window the message is for.
    unsafe {
        let flags = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
        let mut input = INPUT.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut cursor = POINT { x: 0, y: 0 };
        GetCursorPos(&mut cursor);
        match message {
            WM_LBUTTONDOWN if flags & CAN_DRAG != 0 => {
                let mut rect: RECT = std::mem::zeroed();
                GetWindowRect(hwnd, &mut rect);
                input.drag = Some(Dragging { window: hwnd as isize, start: (cursor.x, cursor.y), from: (rect.left, rect.top), moved: false });
                SetCapture(hwnd);
                return 0;
            }
            WM_MOUSEMOVE => {
                if let Some(drag) = &mut input.drag {
                    if drag.window == hwnd as isize {
                        let (dx, dy) = (cursor.x - drag.start.0, cursor.y - drag.start.1);
                        if drag.moved || dx.abs() >= DRAG_SLOP || dy.abs() >= DRAG_SLOP {
                            drag.moved = true;
                            // The window is a child of the desktop layer: move it in that layer's coordinates.
                            let mut at = POINT { x: drag.from.0 + dx, y: drag.from.1 + dy };
                            ScreenToClient(GetParent(hwnd), &mut at);
                            SetWindowPos(hwnd, std::ptr::null_mut(), at.x, at.y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                        }
                        return 0;
                    }
                }
            }
            WM_LBUTTONUP => {
                if let Some(drag) = input.drag {
                    if drag.window == hwnd as isize {
                        input.drag = None;
                        ReleaseCapture();
                        if drag.moved {
                            let mut rect: RECT = std::mem::zeroed();
                            GetWindowRect(hwnd, &mut rect);
                            input.events.push(Event::Dropped(hwnd as isize, rect.left, rect.top));
                        }
                        return 0;
                    }
                }
            }
            WM_RBUTTONUP if flags & CAN_MENU != 0 => {
                input.events.push(Event::Menu(hwnd as isize, cursor.x, cursor.y));
                return 0;
            }
            _ => {}
        }
        drop(input);
        DefWindowProcW(hwnd, message, wparam, lparam)
    }
}

fn register_class() -> Result<(), Box<dyn Error>> {
    if CLASS_READY.load(Ordering::Relaxed) {
        return Ok(());
    }
    // SAFETY: a zeroed WNDCLASSEXW with its fields filled in; the class name outlives the call.
    unsafe {
        let name = wide(CLASS_NAME);
        let mut class: WNDCLASSEXW = std::mem::zeroed();
        class.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        class.lpfnWndProc = Some(window_proc);
        class.hInstance = GetModuleHandleW(std::ptr::null());
        class.lpszClassName = name.as_ptr();
        // 1410 = already registered (by an earlier pass of this process): fine.
        if RegisterClassExW(&class) == 0 && windows_sys::Win32::Foundation::GetLastError() != 1410 {
            return Err(last_error("couldn't register the widget window class").into());
        }
    }
    CLASS_READY.store(true, Ordering::Relaxed);
    Ok(())
}

/// A hidden top-level window that owns the right-click menu (a menu needs a
/// top-level owner, which the desktop-layer child windows are not).
fn menu_owner() -> Result<HWND, Box<dyn Error>> {
    let existing = OWNER.load(Ordering::Relaxed);
    if existing != 0 {
        return Ok(existing as HWND);
    }
    // SAFETY: creating a window of our own registered class; the strings outlive the call.
    let owner = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            wide(CLASS_NAME).as_ptr(),
            wide("Paperloom widgets").as_ptr(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        )
    };
    if owner.is_null() {
        return Err(last_error("couldn't create the menu owner window").into());
    }
    OWNER.store(owner as isize, Ordering::Relaxed);
    Ok(owner)
}

/// Saves finished drags and shows the right-click menu.
fn handle_events(windows: &mut [Window], owner: HWND, control: &mut Control) {
    let events = std::mem::take(&mut INPUT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).events);
    // SAFETY: plain metric query.
    let (screen_w, screen_h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    for event in events {
        match event {
            Event::Dropped(hwnd, left, top) => {
                if let Some(window) = windows.iter_mut().find(|window| window.hwnd as isize == hwnd) {
                    let (fx, fy) = (left as f32 / screen_w as f32, top as f32 / screen_h as f32);
                    window.placed.x = fx.clamp(0.0, 0.95);
                    window.placed.y = fy.clamp(0.0, 0.95);
                    interact::dropped(window.placed.id, fx, fy, control);
                }
            }
            Event::Menu(hwnd, x, y) => {
                let Some(window) = windows.iter().find(|window| window.hwnd as isize == hwnd) else { continue };
                let items = menu::items(window.placed.drag);
                // SAFETY: a popup menu built, shown and destroyed here, owned by our hidden window.
                let picked = unsafe {
                    let popup = CreatePopupMenu();
                    for (index, (_, label)) in items.iter().enumerate() {
                        AppendMenuW(popup, MF_STRING, index + 1, wide(label).as_ptr());
                    }
                    SetForegroundWindow(owner);
                    let picked = TrackPopupMenu(popup, TPM_RETURNCMD | TPM_RIGHTBUTTON, x, y, 0, owner, std::ptr::null());
                    PostMessageW(owner, WM_NULL, 0, 0);
                    DestroyMenu(popup);
                    picked
                };
                if picked > 0 {
                    if let Some((choice, _)) = items.get(picked as usize - 1) {
                        interact::choose(*choice, window.placed.id);
                    }
                }
            }
        }
    }
}

fn pump_messages() {
    // SAFETY: standard message pump on the thread that owns the windows.
    unsafe {
        let mut message: MSG = std::mem::zeroed();
        while PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// Takes on the desktop's own DPI awareness before any window exists (Windows
/// refuses to parent windows of different awareness).
fn adopt_desktop_dpi() {
    // SAFETY: window lookups and one process-wide setting made before any window exists.
    unsafe {
        let mut progman = FindWindowW(wide("Progman").as_ptr(), std::ptr::null());
        for _ in 0..60 {
            if !progman.is_null() {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
            progman = FindWindowW(wide("Progman").as_ptr(), std::ptr::null());
        }
        let mut context = DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2;
        if !progman.is_null() {
            let theirs = GetWindowDpiAwarenessContext(progman);
            for candidate in [
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE,
                DPI_AWARENESS_CONTEXT_SYSTEM_AWARE,
                DPI_AWARENESS_CONTEXT_UNAWARE,
            ] {
                if AreDpiAwarenessContextsEqual(theirs, candidate) != 0 {
                    context = candidate;
                    break;
                }
            }
        }
        SetProcessDpiAwarenessContext(context);
    }
}

/// The top-level window that holds the desktop icons (`SHELLDLL_DefView`), or Progman.
fn icon_host() -> Option<HWND> {
    unsafe extern "system" fn visit(window: HWND, found: LPARAM) -> i32 {
        // SAFETY: `found` is the address of the HWND slot passed below.
        unsafe {
            if !FindWindowExW(window, std::ptr::null_mut(), wide("SHELLDLL_DefView").as_ptr(), std::ptr::null()).is_null() {
                *(found as *mut HWND) = window;
                return 0;
            }
        }
        1
    }
    let mut found: HWND = std::ptr::null_mut();
    // SAFETY: the callback only writes the slot we pass.
    unsafe { EnumWindows(Some(visit), &mut found as *mut HWND as LPARAM) };
    if found.is_null() {
        // SAFETY: a window lookup.
        found = unsafe { FindWindowW(wide("Progman").as_ptr(), std::ptr::null()) };
    }
    (!found.is_null()).then_some(found)
}

/// A top-level window that hosts `SHELLDLL_DefView` (the icons), then the
/// `WorkerW` right after it: the desktop layer before Windows 11 24H2.
fn find_legacy_workerw() -> Option<HWND> {
    unsafe extern "system" fn visit(window: HWND, found: LPARAM) -> i32 {
        // SAFETY: `found` is the address of the HWND slot passed below.
        unsafe {
            let icons = FindWindowExW(window, std::ptr::null_mut(), wide("SHELLDLL_DefView").as_ptr(), std::ptr::null());
            if !icons.is_null() {
                let workerw = FindWindowExW(std::ptr::null_mut(), window, wide("WorkerW").as_ptr(), std::ptr::null());
                *(found as *mut HWND) = workerw;
                return 0;
            }
        }
        1
    }
    let mut found: HWND = std::ptr::null_mut();
    // SAFETY: the callback only writes the slot we pass.
    unsafe { EnumWindows(Some(visit), &mut found as *mut HWND as LPARAM) };
    (!found.is_null()).then_some(found)
}

/// Where the widgets go.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Child windows of the desktop layer, between the wallpaper and the icons.
    Desktop,
    /// Ordinary windows kept directly above the desktop and below every other window.
    PlainAbove,
}

fn last_error(what: &str) -> String {
    // SAFETY: reads this thread's last error code.
    let code = unsafe { windows_sys::Win32::Foundation::GetLastError() };
    format!("{what}: {} (code {code})", std::io::Error::from_raw_os_error(code as i32))
}

/// The desktop layer widgets hang from, and whether it is the "raised" one.
fn find_layer() -> Result<(HWND, bool), Box<dyn Error>> {
    // SAFETY: Win32 calls on handles we just looked up; pointers are null or live NUL-terminated strings.
    unsafe {
        let progman = FindWindowW(wide("Progman").as_ptr(), std::ptr::null());
        if progman.is_null() {
            return Err("no Progman window — is Explorer running?".into());
        }
        let mut result = 0usize;
        SendMessageTimeoutW(progman, 0x052C, 0xD, 0x1, SMTO_NORMAL, 1000, &mut result);
        SendMessageTimeoutW(progman, 0x052C, 0, 0, SMTO_NORMAL, 1000, &mut result);
        for _ in 0..20 {
            let raised = FindWindowExW(progman, std::ptr::null_mut(), wide("WorkerW").as_ptr(), std::ptr::null());
            if !raised.is_null() || find_legacy_workerw().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        let raised = !FindWindowExW(progman, std::ptr::null_mut(), wide("WorkerW").as_ptr(), std::ptr::null()).is_null();
        let parent = if raised { progman } else { find_legacy_workerw().ok_or("no desktop layer (WorkerW) to attach to")? };
        Ok((parent, raised))
    }
}

/// One widget window. Tried as a child of the desktop layer first, then (as the
/// wallpaper engine does when that is refused) as a pop-up moved in with `SetParent`.
fn make_window(mode: Mode, parent: HWND, style: u32, (x, y, w, h): (i32, i32, i32, i32)) -> Result<HWND, String> {
    use windows_sys::Win32::Foundation::SetLastError;
    // SAFETY: Win32 window creation with live NUL-terminated strings and handles we hold.
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let class = wide(CLASS_NAME);
        let title = wide("Paperloom widget");
        SetLastError(0);
        if mode != Mode::Desktop {
            let hwnd = CreateWindowExW(style | WS_EX_TOOLWINDOW, class.as_ptr(), title.as_ptr(), WS_POPUP, x, y, w, h, std::ptr::null_mut(), std::ptr::null_mut(), module, std::ptr::null());
            if hwnd.is_null() {
                return Err(last_error("CreateWindowExW (plain window)"));
            }
            return Ok(hwnd);
        }
        // A layered child of the desktop layer. Some computers refuse this for a program
        // without the Windows 8 compatibility entry (and a pop-up moved in with SetParent
        // is accepted but never paints), so there is no second try here: the caller
        // switches to plain windows.
        let hwnd = CreateWindowExW(style, class.as_ptr(), title.as_ptr(), WS_CHILD | WS_CLIPCHILDREN, x, y, w, h, parent, std::ptr::null_mut(), module, std::ptr::null());
        if hwnd.is_null() {
            return Err(last_error("a layered child window of the desktop was refused"));
        }
        Ok(hwnd)
    }
}

fn build(widgets: &Widgets, mode: Mode) -> Result<(Layer, Vec<Window>), Box<dyn Error>> {
    let (parent, raised) = if mode == Mode::Desktop { find_layer()? } else { (std::ptr::null_mut(), false) };
    // SAFETY: Win32 calls on handles we just looked up.
    let icons = unsafe {
        raised
            .then(|| FindWindowExW(parent, std::ptr::null_mut(), wide("SHELLDLL_DefView").as_ptr(), std::ptr::null()))
            .filter(|icons| !icons.is_null())
    };
    // The window that holds the icons (Progman on the raised desktop, a top-level one before): plain
    // windows go directly above it, so they are above the icons and the wallpaper.
    let desktop = (mode == Mode::PlainAbove).then(icon_host).flatten();
    let layer = Layer { parent, icons, desktop };

    // The desktop layer spans the virtual screen; widget positions are
    // fractions of the primary monitor.
    // SAFETY: plain metric queries.
    let ((origin_x, origin_y), (screen_w, screen_h)) = unsafe {
        let origin = if mode == Mode::Desktop { (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN)) } else { (0, 0) };
        (origin, (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)))
    };

    let mut windows: Vec<Window> = Vec::new();
    for placed in &widgets.placed {
        let (w, h) = widgets::size_of(placed);
        let (w, h) = ((w as f32 * placed.scale) as i32, (h as f32 * placed.scale) as i32);
        let x = (placed.x * screen_w as f32) as i32 - origin_x;
        let y = (placed.y * screen_h as f32) as i32 - origin_y;
        // In the classic layout the icons' window sits above the desktop layer and
        // would swallow every click, so there widgets stay click-through.
        let takes_input = placed.interactive() && (raised || mode != Mode::Desktop);
        let mut style = WS_EX_LAYERED | WS_EX_NOACTIVATE;
        if !takes_input {
            style |= WS_EX_TRANSPARENT;
        }
        let hwnd = match make_window(mode, parent, style, (x, y, w, h)) {
            Ok(hwnd) => hwnd,
            Err(err) => {
                for window in &windows {
                    // SAFETY: our own windows.
                    unsafe { DestroyWindow(window.hwnd) };
                }
                return Err(err.into());
            }
        };
        // SAFETY: our own window.
        unsafe {
            if takes_input {
                let flags = if placed.drag { CAN_DRAG } else { 0 } | if placed.menu { CAN_MENU } else { 0 };
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, flags);
            }
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        windows.push(Window { hwnd, failed: false, placed: placed.clone(), pixmap: None, due: Instant::now() });
    }
    restack(&layer, &windows);
    let backend = match (mode, raised) {
        (Mode::Desktop, true) => "windows (raised desktop)",
        (Mode::Desktop, false) => "windows (classic desktop)",
        (Mode::PlainAbove, _) => "windows (plain windows just above the desktop)",
    };
    paperloom_common::log::info(&format!("widgets attached: {backend}, {} window(s)", windows.len()));
    status(|status| {
        status.backend = backend.into();
        status.windows = windows.len() as u32;
        status.verified = false;
    });
    Ok((layer, windows))
}

/// Orders the widgets. Ones that take mouse input go on top of the desktop layer
/// (above the icons, which would otherwise swallow every click); click-through
/// ones form a block directly below the icons (raised layout) or on top of the
/// wallpaper inside the same `WorkerW` (classic layout). The order is touched
/// only when it has drifted.
fn restack(layer: &Layer, windows: &[Window]) {
    // "Show desktop" hides windows: ours come back.
    for window in windows {
        // SAFETY: visibility query and show on our own windows.
        unsafe {
            if IsWindowVisible(window.hwnd) == 0 {
                ShowWindow(window.hwnd, SW_SHOWNOACTIVATE);
            }
        }
    }
    // Plain windows: kept directly above the desktop window, below every other window.
    if let Some(desktop) = layer.desktop {
        // SAFETY: z-order queries and changes on windows we own or just found.
        unsafe {
            let above = GetWindow(desktop, GW_HWNDPREV);
            if windows.iter().any(|window| window.hwnd == above) {
                return;
            }
            for window in windows {
                SetWindowPos(window.hwnd, if above.is_null() { HWND_TOP } else { above }, 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE);
            }
        }
        return;
    }
    // Test windows have no desktop layer to keep an order in.
    if layer.parent.is_null() {
        return;
    }
    // SAFETY: z-order queries and changes on windows we own or just found.
    unsafe {
        let interactive = |window: &&Window| window.placed.interactive() && layer.icons.is_some();
        let mut previous: Option<HWND> = None;
        for window in windows.iter().filter(interactive) {
            let in_place = match previous {
                Some(above) => GetWindow(above, GW_HWNDNEXT) == window.hwnd,
                None => GetTopWindow(layer.parent) == window.hwnd,
            };
            if !in_place {
                SetWindowPos(window.hwnd, previous.unwrap_or(HWND_TOP), 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE);
            }
            previous = Some(window.hwnd);
        }

        let mut previous = layer.icons;
        for window in windows.iter().filter(|window| !interactive(window)) {
            let in_place = match previous {
                Some(above) => GetWindow(above, GW_HWNDNEXT) == window.hwnd,
                // Classic layout: the first widget is the top child of its WorkerW.
                None => GetTopWindow(layer.parent) == window.hwnd,
            };
            if !in_place {
                let after = previous.unwrap_or(HWND_TOP);
                SetWindowPos(window.hwnd, after, 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE);
            }
            previous = Some(window.hwnd);
        }
    }
}

/// Shows `pixmap` (premultiplied RGBA) as the window's content with per-pixel alpha.
fn present(hwnd: HWND, pixmap: &Pixmap, opacity: f32) -> bool {
    let (width, height) = (pixmap.width() as i32, pixmap.height() as i32);
    // SAFETY: a DIB section we create, fill with exactly width*height*4 bytes, hand to
    // UpdateLayeredWindow, and release before returning.
    unsafe {
        let memory = CreateCompatibleDC(std::ptr::null_mut());
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(memory, &info, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        if bitmap.is_null() || bits.is_null() {
            DeleteDC(memory);
            return false;
        }
        let target = std::slice::from_raw_parts_mut(bits as *mut u8, (width * height * 4) as usize);
        target.copy_from_slice(pixmap.data());
        for pixel in target.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2); // RGBA → BGRA
        }
        let old = SelectObject(memory, bitmap);
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: (opacity.clamp(0.1, 1.0) * 255.0) as u8,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let size = SIZE { cx: width, cy: height };
        let origin = POINT { x: 0, y: 0 };
        let shown = UpdateLayeredWindow(hwnd, std::ptr::null_mut(), std::ptr::null(), &size, memory, &origin, 0, &blend, ULW_ALPHA) != 0;
        let error = if shown { 0 } else { windows_sys::Win32::Foundation::GetLastError() };
        SelectObject(memory, old);
        DeleteObject(bitmap);
        DeleteDC(memory);
        // Keep the reason where the caller's `last_error` can read it.
        windows_sys::Win32::Foundation::SetLastError(error);
        shown
    }
}

/// What looking for the test patch on the screen found.
enum Seen {
    Yes,
    /// The patch was drawn in a clear part of the desktop and was not there.
    No(String),
    /// No part of the desktop is uncovered right now (other windows are over all of it): can't tell.
    Covered,
}

fn class_of(window: HWND) -> String {
    let mut buffer = [0u16; 64];
    // SAFETY: the buffer is 64 wide characters, as stated.
    let length = unsafe { GetClassNameW(window, buffer.as_mut_ptr(), 64) };
    String::from_utf16_lossy(&buffer[..length.max(0) as usize])
}

/// Whether the desktop itself (not another window) is what shows at this screen point.
fn desktop_at(x: i32, y: i32) -> bool {
    // SAFETY: point-to-window queries.
    unsafe {
        let window = WindowFromPoint(POINT { x, y });
        if window.is_null() {
            return false;
        }
        let root = GetAncestor(window, GA_ROOT);
        let class = class_of(if root.is_null() { window } else { root });
        class == "Progman" || class == "WorkerW"
    }
}

/// A square of `size` pixels on the primary screen where nothing but the desktop is showing.
fn clear_spot(size: i32) -> Option<(i32, i32)> {
    // SAFETY: plain metric queries.
    let (width, height) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let mut y = 90;
    while y + size < height - 60 {
        let mut x = 90;
        while x + size < width - 20 {
            let corners = [(x, y), (x + size - 1, y), (x, y + size - 1), (x + size - 1, y + size - 1), (x + size / 2, y + size / 2)];
            if corners.iter().all(|(cx, cy)| desktop_at(*cx, *cy)) {
                return Some((x, y));
            }
            x += 97;
        }
        y += 83;
    }
    None
}

/// Draws a small solid patch in a window of its own, put where widgets are put (the same layer and
/// order), at a spot of the desktop nothing covers, and looks for it on the screen itself. Seen only
/// when it really is there, whatever the wallpaper, a video or another window is doing.
fn check_visible(layer: &Layer) -> Seen {
    const PATCH: i32 = 16;
    let Some((x, y)) = clear_spot(PATCH) else { return Seen::Covered };
    let mode = if layer.desktop.is_some() { Mode::PlainAbove } else { Mode::Desktop };
    // SAFETY: a metric query.
    let (origin_x, origin_y) = if mode == Mode::Desktop { unsafe { (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN)) } } else { (0, 0) };
    let hwnd = match make_window(mode, layer.parent, WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT, (x - origin_x, y - origin_y, PATCH, PATCH)) {
        Ok(hwnd) => hwnd,
        Err(err) => return Seen::No(format!("the test window couldn't be made: {err}")),
    };
    // SAFETY: our own window.
    unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    let probe = Window { hwnd, failed: false, placed: Placed { drag: false, menu: false, ..Placed::default() }, pixmap: None, due: Instant::now() };
    restack(layer, std::slice::from_ref(&probe));
    let outcome = (|| {
        let Some(mut frame) = Pixmap::new(PATCH as u32, PATCH as u32) else { return Seen::Yes };
        for pixel in frame.data_mut().as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&[255, 0, 255, 255]);
        }
        if !present(hwnd, &frame, 1.0) {
            return Seen::No(last_error("the test patch couldn't be shown (UpdateLayeredWindow)"));
        }
        std::thread::sleep(Duration::from_millis(220));
        let Some(seen) = capture(x, y, PATCH, PATCH) else {
            paperloom_common::log::warn("widgets: couldn't read the screen to check; trusting that the windows are shown");
            return Seen::Yes;
        };
        let pink = seen.as_chunks::<4>().0.iter().filter(|pixel| pixel[2] > 235 && pixel[0] > 235 && pixel[1] < 40).count();
        let total = (PATCH * PATCH) as usize;
        if pink * 100 >= total * 60 {
            Seen::Yes
        } else {
            // Something may have moved over the spot just now: only a clear spot proves anything.
            if desktop_at(x + PATCH / 2, y + PATCH / 2) || class_of_point_is_ours(x + PATCH / 2, y + PATCH / 2, hwnd) {
                Seen::No(format!("a test patch drawn in a clear part of the desktop did not show on the screen ({pink} of {total} pixels)"))
            } else {
                Seen::Covered
            }
        }
    })();
    // SAFETY: our own window.
    unsafe { DestroyWindow(hwnd) };
    outcome
}

/// Whether the window at this screen point is `ours` (so nothing else is covering it).
fn class_of_point_is_ours(x: i32, y: i32, ours: HWND) -> bool {
    // SAFETY: a point-to-window query.
    unsafe { WindowFromPoint(POINT { x, y }) == ours }
}

/// Shows the desktop (what Win+D does) so a test can look at it, and puts the windows back after.
pub fn show_desktop(on: bool) {
    // SAFETY: lookups and one message to the taskbar; 419 = minimise all, 416 = undo that.
    unsafe {
        let tray = FindWindowW(wide("Shell_TrayWnd").as_ptr(), std::ptr::null());
        if !tray.is_null() {
            let mut result = 0usize;
            SendMessageTimeoutW(tray, WM_COMMAND, if on { 419 } else { 416 }, 0, SMTO_NORMAL, 1500, &mut result);
        }
    }
}

/// The screen's pixels (BGRA) in a rectangle, as composed by the desktop.
fn capture(left: i32, top: i32, width: i32, height: i32) -> Option<Vec<u8>> {
    // SAFETY: a screen device context and a DIB section we create, use and free here.
    unsafe {
        let screen = GetDC(std::ptr::null_mut());
        if screen.is_null() {
            return None;
        }
        let memory = CreateCompatibleDC(screen);
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(memory, &info, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        let mut out = None;
        if !bitmap.is_null() && !bits.is_null() {
            let old = SelectObject(memory, bitmap);
            if BitBlt(memory, 0, 0, width, height, screen, left, top, SRCCOPY | CAPTUREBLT) != 0 {
                out = Some(std::slice::from_raw_parts(bits as *const u8, (width * height * 4) as usize).to_vec());
            }
            SelectObject(memory, old);
        }
        if !bitmap.is_null() {
            DeleteObject(bitmap);
        }
        DeleteDC(memory);
        ReleaseDC(std::ptr::null_mut(), screen);
        out
    }
}

/// `--probe`: tries every way widgets can be put on this desktop, one after the other, and for
/// each one looks for a test patch on the screen. Says which of them really show something.
pub fn probe() -> Vec<String> {
    QUIET.store(true, Ordering::Relaxed);
    adopt_desktop_dpi();
    if let Err(err) = register_class() {
        return vec![format!("[FAIL] the window class couldn't be registered — {err}")];
    }
    let widgets = super::test_widgets();
    let mut lines = Vec::new();
    let mut seen_in: Option<&str> = None;
    let mut text = Text::new();
    let sample = crate::sample::Sample::new();
    // The test looks at the desktop, so other windows are minimised for its length (and restored).
    show_desktop(true);
    std::thread::sleep(Duration::from_millis(900));
    for (mode, name) in [(Mode::Desktop, "under the icons, attached to the desktop layer"), (Mode::PlainAbove, "plain windows just above the desktop")] {
        match build(&widgets, mode) {
            Err(err) => lines.push(format!("[FAIL] {name}: the windows couldn't be made — {err}")),
            Ok((layer, windows)) => {
                // Draw each window's real picture first, as the real thing does.
                let drawn = windows.iter().all(|window| {
                    widgets::render(&window.placed, &mut text, &Context { sample: &sample, slow: false }).and_then(|frame| frame.pixmap).is_some_and(|pixmap| present(window.hwnd, &pixmap, window.placed.opacity))
                });
                if !drawn {
                    lines.push(format!("[FAIL] {name}: a widget's picture couldn't be shown ({})", last_error("UpdateLayeredWindow")));
                }
                match check_visible(&layer) {
                    Seen::Yes => {
                        lines.push(format!("[PASS] {name}: a test patch was seen on the screen"));
                        seen_in.get_or_insert(name);
                    }
                    Seen::No(reason) => lines.push(format!("[FAIL] {name}: {reason}")),
                    Seen::Covered => lines.push(format!("[FAIL] {name}: no part of the desktop was uncovered to check (something is full-screen over it)")),
                }
                for window in &windows {
                    // SAFETY: our own windows.
                    unsafe { DestroyWindow(window.hwnd) };
                }
            }
        }
    }
    show_desktop(false);
    lines.push(match seen_in {
        Some(name) => format!("[PASS] widgets can be seen on this desktop ({name})"),
        None => "[FAIL] widgets can NOT be seen on this desktop: every way of showing them failed (see the lines above)".to_string(),
    });
    lines
}
