//! macOS desktop integration: a borderless window at the *desktop window
//! level* (below Finder's desktop icons, above the system wallpaper), on
//! every Space, ignoring the mouse, with libmpv rendering into its content
//! view via `wid` (mpv takes an `NSView*` there and adds its own subview).
//!
//! The engine runs as an "accessory" app — no Dock icon, no menu bar — and
//! drives AppKit's event loop by hand (`nextEventMatchingMask` with a
//! 200ms timeout) instead of `NSApp.run()`, so the IPC and pause checks fit
//! in the same single-threaded loop as the other backends. Pumping the run
//! loop that way also services the main dispatch queue, which mpv's Cocoa
//! code relies on.
//!
//! Known gap: covers the main display only (one window, one mpv). macOS
//! can't stretch one window across displays when "Displays have separate
//! Spaces" is on (the default), so mirroring X11's span would mean one
//! window + decoder per display.
//!
//! **Never run on a real Mac** — this dev environment is Linux. It's
//! compile-checked against the exact `objc2-app-kit` API locally and built
//! for real in CI, but "compiles" isn't "shows video under the icons"
//! until someone runs it.

use std::error::Error;
use std::ffi::c_void;
use std::time::{Duration, Instant};

use libmpv2::Mpv;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::MainThreadOnly;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSEventMask, NSEventType, NSScreen,
    NSStatusWindowLevel, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask, NSWindowTitleVisibility,
};
use objc2_foundation::{ns_string, MainThreadMarker, NSArray, NSDate, NSDefaultRunLoopMode, NSDictionary, NSNumber, NSRect, NSString};
use paperloom_common::ipc::Listener;
use paperloom_common::Settings;

use crate::player::{self, Flow};

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGWindowLevelForKey(key: i32) -> i32;
    fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> *mut c_void;
    fn CGSessionCopyCurrentDictionary() -> *mut c_void;
}

const DESKTOP_WINDOW_LEVEL_KEY: i32 = 2; // kCGDesktopWindowLevelKey
const WINDOW_LIST_ON_SCREEN_ONLY: u32 = 1 << 0;
const WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS: u32 = 1 << 4;

fn main_thread() -> Result<MainThreadMarker, Box<dyn Error>> {
    MainThreadMarker::new().ok_or_else(|| "the macOS backend must run on the main thread".into())
}

fn start_app(mtm: MainThreadMarker) -> Retained<NSApplication> {
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    app.finishLaunching();
    app
}

fn main_screen_frame(mtm: MainThreadMarker) -> Result<NSRect, Box<dyn Error>> {
    Ok(NSScreen::mainScreen(mtm).ok_or("no display")?.frame())
}

fn new_window(mtm: MainThreadMarker, frame: NSRect, style: NSWindowStyleMask) -> Retained<NSWindow> {
    // SAFETY: standard designated initializer on a freshly allocated window.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(NSWindow::alloc(mtm), frame, style, NSBackingStoreType::Buffered, false)
    };
    // We hold the only strong reference; AppKit mustn't also free it on close.
    // SAFETY: called before the window is ever closed.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setBackgroundColor(Some(&NSColor::blackColor()));
    window
}

fn mpv_for(window: &NSWindow) -> Result<Mpv, Box<dyn Error>> {
    let view = window.contentView().ok_or("window has no content view")?;
    let view_ptr = Retained::as_ptr(&view) as i64;
    Ok(Mpv::with_initializer(|init| {
        init.set_property("wid", view_ptr)?;
        Ok(())
    })?)
}

/// Dispatches every pending AppKit event, waiting up to `wait` for the
/// first one. Returns the types of the events seen (the preview closes on
/// a key or click).
fn pump_events(app: &NSApplication, wait: Duration) -> Vec<NSEventType> {
    let mut seen = Vec::new();
    let mut until = NSDate::dateWithTimeIntervalSinceNow(wait.as_secs_f64());
    // SAFETY: reading an AppKit-provided static string constant.
    let mode = unsafe { NSDefaultRunLoopMode };
    while let Some(event) = app.nextEventMatchingMask_untilDate_inMode_dequeue(NSEventMask::Any, Some(&until), mode, true) {
        seen.push(event.r#type());
        app.sendEvent(&event);
        until = NSDate::distantPast(); // drain the rest without waiting
    }
    seen
}

pub fn run(initial_path: Option<&str>) -> Result<(), Box<dyn Error>> {
    player::validate_cli_path(initial_path)?;
    let mtm = main_thread()?;
    let mut listener = Listener::bind()?;
    let app = start_app(mtm);

    let window = new_window(mtm, main_screen_frame(mtm)?, NSWindowStyleMask::Borderless);
    // SAFETY: plain CoreGraphics level lookup.
    window.setLevel(unsafe { CGWindowLevelForKey(DESKTOP_WINDOW_LEVEL_KEY) } as isize);
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::Stationary | NSWindowCollectionBehavior::IgnoresCycle,
    );
    window.setIgnoresMouseEvents(true);
    window.setHasShadow(false);
    window.orderBack(None);

    let mpv = mpv_for(&window)?;
    player::configure_wallpaper(&mpv)?;
    if let Some(path) = player::startup_wallpaper(initial_path) {
        player::play(&mpv, &path);
    }

    let mut is_paused = false;
    let mut last_check = Instant::now();
    loop {
        pump_events(&app, Duration::from_millis(200));
        if player::poll_ipc(&mut listener, &mpv) == Flow::Quit {
            window.close();
            return Ok(());
        }
        if last_check.elapsed() >= Duration::from_secs(2) {
            last_check = Instant::now();
            let should_pause = should_pause(&paperloom_common::load(), mtm);
            if should_pause != is_paused {
                is_paused = should_pause;
                player::set_paused(&mpv, is_paused);
            }
        }
    }
}

// --- Pause triggers -------------------------------------------------------

fn should_pause(settings: &Settings, mtm: MainThreadMarker) -> bool {
    let (fullscreen, covered) = if settings.pause_on_fullscreen_app || settings.pause_on_window_covering_desktop {
        window_coverage(mtm)
    } else {
        (false, false)
    };
    (settings.pause_on_fullscreen_app && fullscreen)
        || (settings.pause_on_window_covering_desktop && covered)
        || (settings.pause_on_screen_lock && screen_locked())
        || (settings.pause_on_low_battery && battery_low(settings.low_battery_percent))
}

/// CoreGraphics hands back untyped dictionaries; keys are CFStrings.
type WindowInfo = NSDictionary;

fn get(info: &WindowInfo, key: &NSString) -> Option<Retained<AnyObject>> {
    info.objectForKey(key.as_ref())
}

fn number(info: &WindowInfo, key: &NSString) -> Option<f64> {
    get(info, key)?.downcast::<NSNumber>().ok().map(|n| n.doubleValue())
}

/// `(fullscreen app?, desktop covered?)` for the main display, from the
/// on-screen window list (CoreGraphics' top-left-origin coordinates). A
/// normal-layer window of another app covering the whole display counts as
/// fullscreen; one covering the visible area (display minus menu bar and
/// Dock) counts as covering the desktop — the same approximation as the
/// X11 backend.
fn window_coverage(mtm: MainThreadMarker) -> (bool, bool) {
    let Some(screen) = NSScreen::mainScreen(mtm) else { return (false, false) };
    let full = screen.frame();
    let visible = screen.visibleFrame();
    // AppKit is bottom-left origin; flip the visible area into CG's top-left space.
    let visible_top = full.size.height - (visible.origin.y + visible.size.height);

    // SAFETY: a "Copy" function, so we own the returned array (toll-free
    // bridged CFArray of CFDictionary == NSArray of NSDictionary).
    let list = unsafe { CGWindowListCopyWindowInfo(WINDOW_LIST_ON_SCREEN_ONLY | WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS, 0) };
    if list.is_null() {
        return (false, false);
    }
    // SAFETY: see above — non-null, +1 retained, correct bridged type.
    let windows: Retained<NSArray<WindowInfo>> = unsafe { Retained::from_raw(list.cast()) }.expect("non-null");

    let our_pid = std::process::id() as f64;
    let (mut fullscreen, mut covered) = (false, false);
    for info in windows.iter() {
        if number(&info, ns_string!("kCGWindowLayer")) != Some(0.0) || number(&info, ns_string!("kCGWindowOwnerPID")) == Some(our_pid) {
            continue;
        }
        let Some(bounds) = get(&info, ns_string!("kCGWindowBounds")).and_then(|b| b.downcast::<WindowInfo>().ok()) else {
            continue;
        };
        let get = |key: &NSString| number(&bounds, key).unwrap_or(0.0);
        let (x, y, w, h) = (get(ns_string!("X")), get(ns_string!("Y")), get(ns_string!("Width")), get(ns_string!("Height")));
        if x <= 0.0 && y <= 0.0 && w >= full.size.width && h >= full.size.height {
            fullscreen = true;
        }
        if x <= visible.origin.x
            && y <= visible_top
            && x + w >= visible.origin.x + visible.size.width
            && y + h >= visible_top + visible.size.height
        {
            covered = true;
        }
    }
    (fullscreen, covered)
}

fn screen_locked() -> bool {
    // SAFETY: "Copy" function, owned result (CFDictionary == NSDictionary).
    let session = unsafe { CGSessionCopyCurrentDictionary() };
    if session.is_null() {
        return false;
    }
    // SAFETY: non-null, +1 retained, correct bridged type.
    let session: Retained<WindowInfo> = unsafe { Retained::from_raw(session.cast()) }.expect("non-null");
    get(&session, ns_string!("CGSSessionScreenIsLocked"))
        .and_then(|value| value.downcast::<NSNumber>().ok())
        .is_some_and(|value| value.boolValue())
}

/// `pmset -g batt` — e.g. "Now drawing from 'Battery Power'" then
/// " -InternalBattery-0 (id=…) 18%; discharging; 0:42 remaining". Shelled
/// out every ~2s, the same tradeoff the Linux backend makes for `gdbus`.
fn battery_low(threshold_percent: u8) -> bool {
    let Ok(out) = std::process::Command::new("pmset").args(["-g", "batt"]).output() else {
        return false;
    };
    parse_pmset(&String::from_utf8_lossy(&out.stdout)).is_some_and(|(percent, on_battery)| on_battery && percent <= threshold_percent)
}

fn parse_pmset(output: &str) -> Option<(u8, bool)> {
    let on_battery = output.contains("'Battery Power'");
    let percent = output.split(['\t', ' ', ';']).find_map(|token| token.strip_suffix('%')?.parse::<u8>().ok())?;
    Some((percent, on_battery))
}

// --- Preview --------------------------------------------------------------

/// Full-screen preview (`--preview <file>`): a normal (focusable) window
/// above the menu bar and Dock covering the main display; any key or click
/// closes it. Titled-but-invisible-titlebar rather than borderless, since
/// borderless windows can't become key and would never see a key press.
pub fn preview(path: &str) -> Result<(), Box<dyn Error>> {
    player::validate_cli_path(Some(path))?;
    let mtm = main_thread()?;
    let app = start_app(mtm);

    let window = new_window(mtm, main_screen_frame(mtm)?, NSWindowStyleMask::Titled | NSWindowStyleMask::FullSizeContentView);
    window.setTitlebarAppearsTransparent(true);
    window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
    window.setLevel(NSStatusWindowLevel);
    window.setFrame_display(main_screen_frame(mtm)?, true);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    window.makeKeyAndOrderFront(None);

    let mpv = mpv_for(&window)?;
    mpv.set_property("loop-file", "inf".to_string())?;
    mpv.set_property("osc", false)?;
    mpv.set_property("input-default-bindings", false)?;
    mpv.set_property("input-vo-keyboard", false)?;
    mpv.set_property("cursor-autohide", "always".to_string())?;
    mpv.set_property("mute", true)?;
    mpv.set_property("panscan", 1.0)?;
    mpv.command("loadfile", &[path])?;

    loop {
        let events = pump_events(&app, Duration::from_millis(50));
        let closing = events.iter().any(|kind| matches!(*kind, NSEventType::KeyDown | NSEventType::LeftMouseDown | NSEventType::RightMouseDown));
        if closing || !window.isVisible() {
            window.close();
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_pmset;

    #[test]
    fn parses_pmset_on_battery() {
        let sample = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=4653155)\t18%; discharging; 0:42 remaining present: true\n";
        assert_eq!(parse_pmset(sample), Some((18, true)));
    }

    #[test]
    fn parses_pmset_on_ac() {
        let sample = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=4653155)\t100%; charged; 0:00 remaining present: true\n";
        assert_eq!(parse_pmset(sample), Some((100, false)));
    }
}
