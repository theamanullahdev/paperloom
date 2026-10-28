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
    GW_HWNDNEXT, HWND_TOP, MSG, PM_REMOVE, SMTO_NORMAL, SM_CXSCREEN, SM_CYSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SWP_NOACTIVATE,
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
/// How often the stacking order is verified.
const RESTACK_EVERY: Duration = Duration::from_secs(5);

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

struct Window {
    hwnd: HWND,
    placed: Placed,
    pixmap: Option<Pixmap>,
    due: Instant,
}

/// Where the windows hang: the desktop layer and, on the raised desktop, the
/// icon view they must stay just below.
struct Layer {
    parent: HWND,
    icons: Option<HWND>,
}

pub fn run(widgets: &Widgets, text: &mut Text, control: &mut Control) -> Result<(), Box<dyn Error>> {
    adopt_desktop_dpi();
    register_class()?;
    let owner = menu_owner()?;

    loop {
        let (layer, mut windows) = match build(widgets) {
            Ok(built) => built,
            Err(err) => {
                paperloom_common::log::warn(&format!("widgets: {err}; trying again"));
                std::thread::sleep(Duration::from_secs(3));
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
                present(window.hwnd, &frame.pixmap, window.placed.opacity);
                window.due = now + frame.next_change;
                window.pixmap = Some(frame.pixmap);
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
    // SAFETY: a zeroed WNDCLASSEXW with its fields filled in; the class name outlives the call.
    unsafe {
        let name = wide(CLASS_NAME);
        let mut class: WNDCLASSEXW = std::mem::zeroed();
        class.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        class.lpfnWndProc = Some(window_proc);
        class.hInstance = GetModuleHandleW(std::ptr::null());
        class.lpszClassName = name.as_ptr();
        if RegisterClassExW(&class) == 0 {
            return Err("couldn't register the widget window class".into());
        }
    }
    Ok(())
}

/// A hidden top-level window that owns the right-click menu (a menu needs a
/// top-level owner, which the desktop-layer child windows are not).
fn menu_owner() -> Result<HWND, Box<dyn Error>> {
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
        return Err("couldn't create the menu owner window".into());
    }
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

fn build(widgets: &Widgets) -> Result<(Layer, Vec<Window>), Box<dyn Error>> {
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
        let icons = raised
            .then(|| FindWindowExW(progman, std::ptr::null_mut(), wide("SHELLDLL_DefView").as_ptr(), std::ptr::null()))
            .filter(|icons| !icons.is_null());
        let layer = Layer { parent, icons };

        // The desktop layer spans the virtual screen; widget positions are
        // fractions of the primary monitor.
        let (origin_x, origin_y) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
        let (screen_w, screen_h) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));

        let mut windows = Vec::new();
        for placed in &widgets.placed {
            let (w, h) = widgets::size_of(placed);
            let (w, h) = ((w as f32 * placed.scale) as i32, (h as f32 * placed.scale) as i32);
            let x = (placed.x * screen_w as f32) as i32 - origin_x;
            let y = (placed.y * screen_h as f32) as i32 - origin_y;
            // In the classic layout the icons' window sits above the desktop layer and
            // would swallow every click, so there widgets stay click-through.
            let takes_input = placed.interactive() && raised;
            let mut style = WS_EX_LAYERED | WS_EX_NOACTIVATE;
            if !takes_input {
                style |= WS_EX_TRANSPARENT;
            }
            let hwnd = CreateWindowExW(
                style,
                wide(CLASS_NAME).as_ptr(),
                wide("Paperloom widget").as_ptr(),
                WS_CHILD | WS_CLIPCHILDREN,
                x,
                y,
                w,
                h,
                parent,
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            );
            if hwnd.is_null() {
                return Err("couldn't create a widget window".into());
            }
            if takes_input {
                let flags = if placed.drag { CAN_DRAG } else { 0 } | if placed.menu { CAN_MENU } else { 0 };
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, flags);
            }
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            windows.push(Window { hwnd, placed: placed.clone(), pixmap: None, due: Instant::now() });
        }
        restack(&layer, &windows);
        paperloom_common::log::info(&format!("widgets attached ({} layout)", if raised { "raised" } else { "classic" }));
        Ok((layer, windows))
    }
}

/// Orders the widgets. Ones that take mouse input go on top of the desktop layer
/// (above the icons, which would otherwise swallow every click); click-through
/// ones form a block directly below the icons (raised layout) or on top of the
/// wallpaper inside the same `WorkerW` (classic layout). The order is touched
/// only when it has drifted.
fn restack(layer: &Layer, windows: &[Window]) {
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
fn present(hwnd: HWND, pixmap: &Pixmap, opacity: f32) {
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
            return;
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
        UpdateLayeredWindow(hwnd, std::ptr::null_mut(), std::ptr::null(), &size, memory, &origin, 0, &blend, ULW_ALPHA);
        SelectObject(memory, old);
        DeleteObject(bitmap);
        DeleteDC(memory);
    }
}
