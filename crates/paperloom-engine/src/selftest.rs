//! `paperloom-engine --selftest [clip.mp4]`: runs the engine's own startup
//! steps one by one and prints what each did — the player library, hardware
//! decoding, and (on Windows) the desktop window tree and every way of
//! attaching behind the icons. The app's "Run all tests" button runs this and
//! pastes the output into its report, so a machine we can't touch still tells
//! us exactly which step fails. Lines look like `[PASS] name: detail`.

use std::io::Write;
use std::time::{Duration, Instant};

use libmpv2::Mpv;

pub fn line(kind: &str, text: impl AsRef<str>) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "[{kind}] {}", text.as_ref());
    let _ = out.flush();
}

pub fn pass(text: impl AsRef<str>) {
    line("PASS", text);
}
pub fn fail(text: impl AsRef<str>) {
    line("FAIL", text);
}
pub fn info(text: impl AsRef<str>) {
    line("INFO", text);
}

pub fn run(clip: Option<&str>) {
    crate::player::SELFTEST.store(true, std::sync::atomic::Ordering::Relaxed);
    info(format!("engine {} on {} / {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH));
    info(format!("executable {}", std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default()));

    let clip = clip.filter(|path| std::path::Path::new(path).is_file());
    match clip {
        Some(path) => info(format!("test clip {path}")),
        None => fail("test clip: none given or missing, so playback steps are skipped"),
    }

    // 1. libmpv loads and starts (no window needed).
    match Mpv::with_initializer(|init| {
        init.set_property("vo", "null")?;
        init.set_property("ao", "null")?;
        Ok(())
    }) {
        Ok(mpv) => {
            pass("libmpv starts headless");
            info(format!("mpv {}", text(&mpv, "mpv-version")));
            info(format!("ffmpeg {}", text(&mpv, "ffmpeg-version")));
            if let Some(path) = clip {
                decode_test(&mpv, path);
            }
        }
        Err(err) => fail(format!("libmpv could not start: {err} (is libmpv-2.dll next to the program?)")),
    }

    #[cfg(windows)]
    crate::backends::windows::selftest(clip);
    #[cfg(not(windows))]
    info("desktop attach: tested by actually setting a wallpaper on this platform");

    info("selftest finished");
}

pub fn text(mpv: &Mpv, name: &str) -> String {
    mpv.get_property::<String>(name).unwrap_or_else(|_| "?".into())
}

/// Plays `path` for a few seconds into a null video output and reports that
/// time moves (decoding works) and what decoder was used.
fn decode_test(mpv: &Mpv, path: &str) {
    let _ = mpv.set_property("hwdec", "auto-safe".to_string());
    if let Err(err) = mpv.command("loadfile", &[path]) {
        fail(format!("decode: loadfile failed: {err}"));
        return;
    }
    let started = Instant::now();
    let mut position = 0.0;
    while started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(250));
        position = mpv.get_property::<f64>("time-pos").unwrap_or(0.0);
        if position > 1.0 {
            break;
        }
    }
    if position > 0.5 {
        pass(format!("decode: playing ({position:.1}s in {} ms)", started.elapsed().as_millis()));
    } else {
        fail("decode: the clip did not start playing within 5 seconds");
    }
    info(format!(
        "decode details: codec {}, {}x{}, hwdec {}",
        text(mpv, "video-codec"),
        mpv.get_property::<i64>("width").unwrap_or(0),
        mpv.get_property::<i64>("height").unwrap_or(0),
        text(mpv, "hwdec-current"),
    ));
}
