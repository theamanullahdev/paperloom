//! `paperloom-widgets --selftest`: checks, without touching the desktop, that
//! this computer can do what widgets need — read the settings, find the looks,
//! load fonts, draw every look of every kind, and open the lock file — and
//! prints one line each, `[PASS]` or `[FAIL]`. The developer page runs it.

use std::process::ExitCode;
use std::time::Instant;

use paperloom_common::widgets::{self, Widgets, BUILT_IN};

use crate::widgets::{self as draw, Context};

pub fn run() -> ExitCode {
    let mut failures = 0;
    let mut step = |name: &str, work: &mut dyn FnMut() -> Result<String, String>| {
        let started = Instant::now();
        match work() {
            Ok(detail) => println!("[PASS] {name} — {detail} ({} ms)", started.elapsed().as_millis()),
            Err(detail) => {
                failures += 1;
                println!("[FAIL] {name} — {detail} ({} ms)", started.elapsed().as_millis());
            }
        }
    };

    step("settings and placed widgets read", &mut || {
        let widgets = Widgets::load();
        Ok(format!("{} placed, widgets {}", widgets.placed.len(), if widgets.enabled { "on" } else { "off" }))
    });
    paperloom_skin::set_user_dir(&paperloom_common::skins_dir());
    step("looks found", &mut || {
        let all = paperloom_skin::all();
        let installed = all.iter().filter(|skin| skin.dir.is_some()).count();
        if all.is_empty() {
            Err("no looks at all".into())
        } else {
            Ok(format!("{} looks ({installed} installed)", all.len()))
        }
    });
    let mut text = crate::render::Text::new();
    let sample = crate::sample::Sample::new();
    step("every look of every kind draws", &mut || {
        let mut drawn = 0;
        for entry in BUILT_IN {
            for skin in paperloom_skin::for_kind(entry.kind) {
                let placed = widgets::Placed { id: 1, kind: entry.kind.to_string(), skin: skin.id.clone(), ..widgets::Placed::default() };
                let frame = draw::render(&placed, &mut text, &Context { sample: &sample, slow: false }).ok_or_else(|| format!("{} / {} gave nothing", entry.kind, skin.id))?;
                let pixmap = frame.pixmap.ok_or_else(|| format!("{} / {} drew nothing", entry.kind, skin.id))?;
                if pixmap.pixels().iter().all(|pixel| pixel.alpha() == 0) {
                    return Err(format!("{} / {} is fully transparent", entry.kind, skin.id));
                }
                drawn += 1;
            }
        }
        draw::forget();
        Ok(format!("{drawn} pictures"))
    });
    step("readings (processor, memory, disk, network, battery)", &mut || {
        Ok(format!(
            "cpu {:.0}%, memory {} of {} MB, disk {} of {} GB, battery {}",
            sample.cpu,
            sample.memory_used / 1_048_576,
            sample.memory_total / 1_048_576,
            sample.disk_used / 1_073_741_824,
            sample.disk_total / 1_073_741_824,
            sample.battery.map_or("none".to_string(), |battery| format!("{}%{}", battery.percent, if battery.discharging { " (discharging)" } else { "" }))
        ))
    });
    step("the widgets lock file can be opened", &mut || {
        let path = paperloom_common::widgets_lock();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path).map(|_| path.display().to_string()).map_err(|err| format!("{}: {err}", path.display()))
    });
    step("the process is running", &mut || Ok(if widgets::is_running() { "yes".to_string() } else { "no (that is fine for a self-test)".to_string() }));

    println!("{}", if failures == 0 { "All checks passed." } else { "Some checks failed." });
    if failures == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
