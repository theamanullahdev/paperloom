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
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, ScreenToClient, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER,
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
    GW_HWNDNEXT, GW_HWNDPREV, HWND_TOP, MSG, PM_REMOVE, SMTO_NORMAL, SM_CXSCREEN, SM_CYSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SWP_NOACTIVATE,
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
/// How often the stacking order is verified.
const RESTACK_EVERY: Duration = Duration::from_secs(5);

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
    /// Plain windows: the desktop window (Progman) they are kept directly above.
    desktop: Option<HWND>,
}

pub fn run(widgets: &Widgets, text: &mut Text, control: &mut Control) -> Result<(), Box<dyn Error>> {
    adopt_desktop_dpi();
    register_class()?;
    let owner = menu_owner()?;

    loop {
        // Test mode shows plain windows on top, to prove drawing works even where
        // attaching to the desktop doesn't; otherwise the desktop first, and
        // plain windows beneath everything else if the desktop refuses us.
        let first = if control.testing() {
            Mode::PlainTop
        } else if DESKTOP_UNSUPPORTED.load(Ordering::Relaxed) || paperloom_common::dev::DevSwitches::active().widgets_plain_windows {
            Mode::PlainAbove
        } else {
            Mode::Desktop
        };
        let built = build(widgets, first).or_else(|err| {
            if first != Mode::Desktop {
                return Err(err);
            }
            DESKTOP_UNSUPPORTED.store(true, Ordering::Relaxed);
            paperloom_common::log::warn(&format!("widgets: couldn't attach to the desktop ({err}); using plain windows just above it instead"));
            paperloom_common::widgets::Status::update(|status| status.problem = format!("couldn't attach to the desktop: {err}"));
            build(widgets, Mode::PlainAbove)
        });
        let (layer, mut windows) = match built {
            Ok(built) => built,
            Err(err) => {
                paperloom_common::log::warn(&format!("widgets: {err}; trying again"));
                paperloom_common::widgets::Status::update(|status| status.problem = err.to_string());
                std::thread::sleep(Duration::from_secs(3));
                if control.changed() {
                    return Ok(());
                }
                continue;
            }
        };
        let mut next_restack = Instant::now() + RESTACK_EVERY;
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
                    paperloom_common::widgets::Status::update(|status| status.problem = format!("a widget picture couldn't be shown ({why})"));
                    // Attached to the desktop but nothing can be painted there: use plain windows instead.
                    if layer.parent.is_null() {
                        continue;
                    }
                    DESKTOP_UNSUPPORTED.store(true, Ordering::Relaxed);
                    break 'alive;
                }
                window.pixmap = Some(pixmap);
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
    /// Ordinary windows above everything (the developer test).
    PlainTop,
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
    use windows_sys::Win32::UI::WindowsAndMessaging::{HWND_TOPMOST, WS_EX_TOPMOST};
    // SAFETY: Win32 window creation with live NUL-terminated strings and handles we hold.
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let class = wide(CLASS_NAME);
        let title = wide("Paperloom widget");
        SetLastError(0);
        if mode != Mode::Desktop {
            let top = if mode == Mode::PlainTop { WS_EX_TOPMOST } else { 0 };
            let hwnd = CreateWindowExW(style | WS_EX_TOOLWINDOW | top, class.as_ptr(), title.as_ptr(), WS_POPUP, x, y, w, h, std::ptr::null_mut(), std::ptr::null_mut(), module, std::ptr::null());
            if hwnd.is_null() {
                return Err(last_error("CreateWindowExW (plain window)"));
            }
            if mode == Mode::PlainTop {
                SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE);
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
    // SAFETY: a window lookup.
    let desktop = (mode == Mode::PlainAbove).then(|| unsafe { FindWindowW(wide("Progman").as_ptr(), std::ptr::null()) }).filter(|desktop| !desktop.is_null());
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
        (Mode::PlainTop, _) => "windows (plain windows, on top: test)",
    };
    paperloom_common::log::info(&format!("widgets attached: {backend}, {} window(s)", windows.len()));
    paperloom_common::widgets::Status::update(|status| {
        status.backend = backend.into();
        status.windows = windows.len() as u32;
    });
    Ok((layer, windows))
}

/// Orders the widgets. Ones that take mouse input go on top of the desktop layer
/// (above the icons, which would otherwise swallow every click); click-through
/// ones form a block directly below the icons (raised layout) or on top of the
/// wallpaper inside the same `WorkerW` (classic layout). The order is touched
/// only when it has drifted.
fn restack(layer: &Layer, windows: &[Window]) {
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
