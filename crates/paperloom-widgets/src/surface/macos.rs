//! macOS: a borderless, transparent `NSWindow` per widget at a level between
//! the wallpaper and the desktop icons (so icons draw over it and every normal
//! window covers it), on every Space, never taking focus. Content is an
//! `NSBitmapImageRep` filled from the pixmap; AppKit's event loop is pumped by
//! hand, like the wallpaper engine's macOS backend.

use std::error::Error;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::MainThreadOnly;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSBitmapImageRep, NSColor, NSDeviceRGBColorSpace, NSEventMask, NSImage,
    NSImageView, NSScreen, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSSize};
use tiny_skia::Pixmap;

use paperloom_common::widgets::{Placed, Widgets};

use crate::control::Control;
use crate::render::Text;
use crate::widgets::{self, Context};

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGWindowLevelForKey(key: i32) -> i32;
}

const DESKTOP_WINDOW_LEVEL_KEY: i32 = 2; // kCGDesktopWindowLevelKey: the wallpaper
/// Above the wallpaper's level, far below kCGDesktopIconWindowLevel (which is 20 higher).
const ABOVE_WALLPAPER: isize = 5;

struct Window {
    window: Retained<NSWindow>,
    view: Retained<NSImageView>,
    placed: Placed,
    due: Instant,
}

pub fn run(widgets: &Widgets, text: &mut Text, control: &mut Control) -> Result<(), Box<dyn Error>> {
    let mtm = MainThreadMarker::new().ok_or("the macOS backend must run on the main thread")?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    app.finishLaunching();

    let screen = NSScreen::mainScreen(mtm).ok_or("no display")?.frame();
    let mut windows: Vec<Window> = widgets.placed.iter().map(|placed| open(mtm, screen, placed)).collect();

    while !control.changed() {
        let now = Instant::now();
        let paused = control.paused();
        if !paused {
            control.refresh_sample();
        }
        let slow = control.slow();
        for window in &mut windows {
            if paused || window.due > now {
                continue;
            }
            let Some(frame) = widgets::render(&window.placed.kind, window.placed.scale, text, &Context { sample: &control.sample, slow }) else {
                window.due = now + Duration::from_secs(3600);
                continue;
            };
            show(window, &frame.pixmap);
            window.due = now + frame.next_change;
        }

        let wake = windows.iter().map(|window| window.due).min().unwrap_or(now + Duration::from_secs(1));
        let nap = wake.saturating_duration_since(Instant::now()).min(Duration::from_secs(1));
        let until = NSDate::dateWithTimeIntervalSinceNow(nap.as_secs_f64());
        // SAFETY: reading an AppKit-provided static string constant.
        let mode = unsafe { NSDefaultRunLoopMode };
        while let Some(event) = app.nextEventMatchingMask_untilDate_inMode_dequeue(NSEventMask::Any, Some(&until), mode, true) {
            app.sendEvent(&event);
        }
    }
    for window in &windows {
        window.window.orderOut(None);
    }
    Ok(())
}

fn open(mtm: MainThreadMarker, screen: NSRect, placed: &Placed) -> Window {
    let (w, h) = widgets::size_of(&placed.kind);
    let (w, h) = (f64::from(w) * f64::from(placed.scale), f64::from(h) * f64::from(placed.scale));
    // AppKit's origin is the bottom-left of the screen; the settings' is the top-left.
    let x = screen.origin.x + f64::from(placed.x) * screen.size.width;
    let y = screen.origin.y + screen.size.height - f64::from(placed.y) * screen.size.height - h;
    let frame = NSRect::new(NSPoint::new(x, y), NSSize::new(w, h));

    // SAFETY: standard designated initializer on a freshly allocated window.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(NSWindow::alloc(mtm), frame, NSWindowStyleMask::Borderless, NSBackingStoreType::Buffered, false)
    };
    // SAFETY: called before the window is ever closed; we hold the only strong reference.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setOpaque(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
    window.setHasShadow(false);
    window.setAlphaValue(f64::from(placed.opacity));
    // SAFETY: plain CoreGraphics level lookup.
    window.setLevel(unsafe { CGWindowLevelForKey(DESKTOP_WINDOW_LEVEL_KEY) } as isize + ABOVE_WALLPAPER);
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::Stationary | NSWindowCollectionBehavior::IgnoresCycle,
    );
    // Mouse input isn't handled here yet, so the window stays click-through.
    window.setIgnoresMouseEvents(true);

    let view = NSImageView::initWithFrame(NSImageView::alloc(mtm), NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w, h)));
    window.setContentView(Some(&view));
    window.orderFrontRegardless();
    Window { window, view, placed: placed.clone(), due: Instant::now() }
}

/// Puts the pixmap (premultiplied RGBA, which is what a bitmap rep expects by default) in the window.
fn show(window: &Window, pixmap: &Pixmap) {
    let (width, height) = (pixmap.width() as isize, pixmap.height() as isize);
    // SAFETY: a fresh bitmap rep that owns its buffer (null planes); we copy exactly
    // width*height*4 bytes into it, matching the 8-bit RGBA, 4-bytes-per-pixel layout asked for.
    unsafe {
        let Some(rep) = NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            width,
            height,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            width * 4,
            32,
        ) else {
            return;
        };
        std::ptr::copy_nonoverlapping(pixmap.data().as_ptr(), rep.bitmapData(), pixmap.data().len());
        let image = NSImage::initWithSize(NSImage::alloc(), NSSize::new(width as f64, height as f64));
        image.addRepresentation(&rep);
        window.view.setImage(Some(&image));
    }
    let _ = &window.window;
}
