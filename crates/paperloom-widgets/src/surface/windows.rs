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
use std::time::{Duration, Instant};

use tiny_skia::Pixmap;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{
    AreDpiAwarenessContextsEqual, GetWindowDpiAwarenessContext, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, DPI_AWARENESS_CONTEXT_SYSTEM_AWARE, DPI_AWARENESS_CONTEXT_UNAWARE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumWindows, FindWindowExW, FindWindowW, GetSystemMetrics, GetTopWindow, GetWindow,
    IsWindow, PeekMessageW, RegisterClassExW, SendMessageTimeoutW, SetWindowPos, ShowWindow, TranslateMessage, UpdateLayeredWindow,
    GW_HWNDNEXT, HWND_TOP, MSG, PM_REMOVE, SMTO_NORMAL, SM_CXSCREEN, SM_CYSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SW_SHOWNOACTIVATE, ULW_ALPHA, WNDCLASSEXW, WS_CHILD, WS_CLIPCHILDREN, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TRANSPARENT,
};

use paperloom_common::widgets::{Placed, Widgets};

use crate::control::Control;
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
                let Some(frame) = widgets::render(&window.placed.kind, window.placed.scale, text, &Context { sample: &control.sample, slow }) else {
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
            std::thread::sleep(wake.saturating_duration_since(Instant::now()).min(Duration::from_secs(1)));
        }
        for window in &windows {
            // SAFETY: our own windows; destroying one that is already gone is harmless.
            unsafe { DestroyWindow(window.hwnd) };
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: forwarding the window's own message to the default handler.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
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
            let (w, h) = widgets::size_of(&placed.kind);
            let (w, h) = ((w as f32 * placed.scale) as i32, (h as f32 * placed.scale) as i32);
            let x = (placed.x * screen_w as f32) as i32 - origin_x;
            let y = (placed.y * screen_h as f32) as i32 - origin_y;
            let mut style = WS_EX_LAYERED | WS_EX_NOACTIVATE;
            if placed.click_through {
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
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            windows.push(Window { hwnd, placed: placed.clone(), pixmap: None, due: Instant::now() });
        }
        restack(&layer, &windows);
        paperloom_common::log::info(&format!("widgets attached ({} layout)", if raised { "raised" } else { "classic" }));
        Ok((layer, windows))
    }
}

/// Orders the widgets as a block directly below the icons (raised layout) or
/// on top of the wallpaper inside the same `WorkerW` (classic layout),
/// touching the order only when it has drifted.
fn restack(layer: &Layer, windows: &[Window]) {
    // SAFETY: z-order queries and changes on windows we own or just found.
    unsafe {
        let mut previous = layer.icons;
        for window in windows {
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
