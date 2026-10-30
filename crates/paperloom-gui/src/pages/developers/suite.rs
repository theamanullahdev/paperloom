//! "Run all tests": walks through everything the app does on a computer —
//! files, helpers, video, the wallpaper engine, the network, the sources,
//! updates — and writes one report that says which step fails and why, with
//! the engine's own self-test output and the diagnostics appended. Meant to
//! be copied and sent when something doesn't work on a machine we can't
//! look at.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use paperloom_common::{EngineState, Library, Settings};

pub type Progress = Arc<Mutex<Vec<String>>>;

struct Report {
    lines: Vec<String>,
    failures: Vec<String>,
    progress: Progress,
}

impl Report {
    fn push(&mut self, line: String) {
        self.lines.push(line.clone());
        self.progress.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(line);
    }

    /// Runs one step, timing it, and records PASS/FAIL with its detail.
    fn step(&mut self, name: &str, work: impl FnOnce() -> Result<String, String>) {
        self.progress.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(format!("… {name}"));
        let started = Instant::now();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).unwrap_or_else(|_| Err("the step crashed (panic)".into()));
        let ms = started.elapsed().as_millis();
        let line = match outcome {
            Ok(detail) => format!("[PASS] {name} — {detail} ({ms} ms)"),
            Err(detail) => {
                self.failures.push(format!("{name}: {detail}"));
                format!("[FAIL] {name} — {detail} ({ms} ms)")
            }
        };
        // Replace the "…" placeholder with the result.
        self.progress.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).pop();
        self.push(line);
    }

    fn info(&mut self, line: String) {
        self.push(format!("[INFO] {line}"));
    }
}

pub fn run(settings: &Settings, progress: Progress) -> String {
    progress.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clear();
    let mut report = Report { lines: Vec::new(), failures: Vec::new(), progress };
    let started = Instant::now();

    // --- Files and settings
    report.step("data, config, logs and update folders are writable", || {
        let mut checked = Vec::new();
        for folder in [super::Folder::Data, super::Folder::Config, super::Folder::Logs, super::Folder::Updates, super::Folder::Library] {
            let path = folder.path();
            std::fs::create_dir_all(&path).map_err(|err| format!("{}: {err}", path.display()))?;
            let probe = path.join(".paperloom-write-test");
            std::fs::write(&probe, b"x").map_err(|err| format!("{}: cannot write ({err})", path.display()))?;
            let _ = std::fs::remove_file(&probe);
            checked.push(path.display().to_string());
        }
        Ok(checked.join(", "))
    });
    report.step("settings file reads and writes back unchanged", || {
        paperloom_common::save(settings).map_err(|err| format!("could not save settings: {err}"))?;
        let again = paperloom_common::load();
        (again == *settings).then(|| paperloom_common::config_file().display().to_string()).ok_or_else(|| "values changed on the way".to_string())
    });
    report.step("library manifest loads and its files exist", || {
        let library = Library::load_manifest();
        let missing = library.entries.iter().filter(|entry| !entry.path.is_file()).count();
        if missing == 0 {
            Ok(format!("{} wallpapers", library.entries.len()))
        } else {
            Err(format!("{missing} of {} wallpaper files are missing", library.entries.len()))
        }
    });

    // --- Helper programs
    report.step("the wallpaper engine program is next to the app", || {
        paperloom_common::process::sibling_binary("paperloom-engine").map(|path| path.display().to_string()).ok_or_else(|| "paperloom-engine was not found beside paperloom-gui".to_string())
    });
    if cfg!(windows) {
        for dll in ["libmpv-2.dll", "vulkan-1.dll"] {
            report.step(&format!("{dll} is installed"), || {
                let folder = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|p| p.to_path_buf())).ok_or("no executable folder")?;
                let path = folder.join(dll);
                let size = std::fs::metadata(&path).map(|meta| meta.len()).map_err(|_| format!("{} is missing — reinstall Paperloom", path.display()))?;
                Ok(format!("{} ({} MB)", path.display(), size / 1_048_576))
            });
        }
    }
    report.step("the desktop widgets program is next to the app", || {
        paperloom_common::process::sibling_binary("paperloom-widgets").map(|path| path.display().to_string()).ok_or_else(|| "paperloom-widgets was not found beside paperloom-gui — reinstall Paperloom".to_string())
    });
    report.step("widgets self-test (draws every look, reads the meters)", || {
        let output = super::actions::widgets_selftest()?;
        let failed = output.lines().filter(|line| line.starts_with("[FAIL]")).count();
        if failed == 0 {
            Ok(format!("{} checks passed", output.lines().filter(|line| line.starts_with("[PASS]")).count()))
        } else {
            Err(output.lines().filter(|line| line.starts_with("[FAIL]")).collect::<Vec<_>>().join(" | "))
        }
    });
    report.step("widgets can be seen on the desktop (a test patch is looked for on the screen)", || {
        let output = super::actions::widgets_probe()?;
        if output.lines().any(|line| line.starts_with("[FAIL] widgets can NOT")) {
            return Err(output.lines().filter(|line| line.starts_with("[FAIL]")).collect::<Vec<_>>().join(" | "));
        }
        Ok(output.lines().find(|line| line.starts_with("[PASS] widgets can be seen")).map(|line| line.trim_start_matches("[PASS] ").to_string()).unwrap_or_else(|| output.lines().next().unwrap_or_default().trim_start_matches("[INFO] ").to_string()))
    });
    report.step("widgets status", || {
        let running = paperloom_common::widgets::is_running();
        let status = paperloom_common::widgets::Status::read().unwrap_or_default();
        let widgets = paperloom_common::widgets::Widgets::load();
        let line = format!(
            "{} · {} placed, widgets {} · draws on: {} · seen on screen: {} · {}",
            if running { "running" } else { "not running" },
            widgets.placed.len(),
            if widgets.enabled { "on" } else { "off" },
            if status.backend.is_empty() { "—" } else { &status.backend },
            if status.verified { "yes" } else { "no" },
            if status.paused.is_empty() { "not hidden".to_string() } else { format!("hidden: {}", status.paused) }
        );
        // Running, switched on and not hidden on purpose, yet nothing was seen: that is a failure.
        let should_show = running && widgets.enabled && status.paused.is_empty();
        if !status.problem.is_empty() || (should_show && !status.verified && cfg!(windows)) {
            Err(format!("{line} · problem: {}", if status.problem.is_empty() { "widgets are on but nothing was seen on the screen yet" } else { &status.problem }))
        } else {
            Ok(line)
        }
    });
    report.step("ffmpeg runs", || {
        let output = paperloom_common::process::command("ffmpeg").arg("-version").output().map_err(|err| format!("could not start ffmpeg: {err}"))?;
        String::from_utf8_lossy(&output.stdout).lines().next().map(str::to_string).ok_or_else(|| "ffmpeg printed nothing".to_string())
    });

    // --- Video
    let clip = super::actions::make_test_clip();
    match &clip {
        Ok(path) => report.step("a test video can be made", || Ok(path.display().to_string())),
        Err(err) => report.step("a test video can be made", || Err(err.clone())),
    }
    if let Ok(path) = &clip {
        report.step("the video is read back (probe)", || {
            let info = paperloom_common::media::probe(path).ok_or("ffmpeg could not read the test video")?;
            Ok(format!("{} {} {}x{} {:.0} fps", info.codec, info.pixel_format, info.width, info.height, info.fps))
        });
        report.step("a video can be re-encoded (libx264)", || {
            let info = paperloom_common::media::probe(path).ok_or("probe failed")?;
            let plan = paperloom_common::media::Plan { scale_to: Some((640, 360)), fps_cap: Some(24), change_format: true };
            let out = path.with_file_name("paperloom-test-encoded.mp4");
            let status = paperloom_common::process::command("ffmpeg")
                .args(paperloom_common::media::encode_args(&plan, &info, path, &out))
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .output()
                .map_err(|err| err.to_string())?;
            let size = std::fs::metadata(&out).map(|meta| meta.len()).unwrap_or(0);
            let _ = std::fs::remove_file(&out);
            if status.status.success() && size > 0 {
                Ok(format!("{size} bytes"))
            } else {
                Err(String::from_utf8_lossy(&status.stderr).lines().last().unwrap_or("ffmpeg failed").to_string())
            }
        });
        report.step("a preview picture can be taken from a video", || {
            let out = path.with_file_name("paperloom-test-frame.jpg");
            let status = paperloom_common::process::command("ffmpeg")
                .args(["-y", "-ss", "1", "-i"])
                .arg(path)
                .args(["-frames:v", "1"])
                .arg(&out)
                .output()
                .map_err(|err| err.to_string())?;
            let size = std::fs::metadata(&out).map(|meta| meta.len()).unwrap_or(0);
            let _ = std::fs::remove_file(&out);
            (status.status.success() && size > 0).then(|| format!("{size} bytes")).ok_or_else(|| "no picture was written".to_string())
        });
    }

    // --- The wallpaper engine
    let engine_output = clip.as_ref().ok().map(|path| engine_selftest(path));
    if let Some(Ok(text)) = &engine_output {
        let failed: Vec<&str> = text.lines().filter(|line| line.starts_with("[FAIL]")).collect();
        report.step("engine self-test (its own steps are listed below)", || {
            if failed.is_empty() {
                Ok("every engine step passed".into())
            } else {
                Err(format!("{} engine step(s) failed: {}", failed.len(), failed.iter().map(|line| line.trim_start_matches("[FAIL] ")).collect::<Vec<_>>().join(" | ")))
            }
        });
    } else if let Some(Err(err)) = &engine_output {
        report.step("engine self-test (its own steps are listed below)", || Err(err.clone()));
    }
    if let Ok(path) = &clip {
        engine_roundtrip(&mut report, path);
    }

    // --- Network and sources
    let hosts = crate::pages::network::hosts_for(settings);
    report.step("internet connection", || {
        let diagnosis = paperloom_fetch::diagnose(&hosts);
        let detail = format!(
            "{:?}, internet {} ms; {}",
            diagnosis.verdict,
            diagnosis.internet_ms.map_or("-".into(), |ms| ms.to_string()),
            diagnosis.hosts.iter().map(|host| format!("{} dns {} ms connect {} ms", host.host, host.dns_ms.map_or("FAILED".into(), |ms| ms.to_string()), host.connect_ms.map_or("FAILED".into(), |ms| ms.to_string()))).collect::<Vec<_>>().join("; ")
        );
        matches!(diagnosis.verdict, paperloom_fetch::Verdict::Good | paperloom_fetch::Verdict::Slow).then_some(detail.clone()).ok_or(detail)
    });
    for source in settings.sources.iter().filter(|source| source.enabled) {
        let needs_key = matches!(source.id.as_str(), paperloom_common::SOURCE_PEXELS | paperloom_common::SOURCE_PIXABAY);
        if needs_key && source.api_key.is_empty() {
            report.info(format!("source {} skipped: no API key set", source.name));
            continue;
        }
        let source = source.clone();
        report.step(&format!("source {} answers a search", source.name), move || {
            let page = paperloom_fetch::fetch_page(std::slice::from_ref(&source), "nature", 1, &Default::default(), false).map_err(|err| err.to_string())?;
            if page.items.is_empty() {
                return Err(format!("no results ({})", page.notices.join("; ")));
            }
            let first = &page.items[0];
            let thumb = paperloom_fetch::fetch_thumbnail(&first.thumb_url).map(|_| "preview image downloads").unwrap_or("preview image FAILED");
            Ok(format!("{} results; first “{}”; {thumb}", page.items.len(), first.title))
        });
    }
    report.step("update check", || {
        let check = paperloom_fetch::update::check(false).map_err(|err| err.to_string())?;
        Ok(format!("{} releases listed via {}", check.releases.len(), check.api_url))
    });
    report.info(format!("start at login: {}", if paperloom_common::autostart::is_enabled() { "on" } else { "off" }));

    // --- Assemble
    let passed = report.lines.iter().filter(|line| line.starts_with("[PASS]")).count();
    let mut out = String::new();
    out.push_str(&format!("PAPERLOOM SELF-TEST  {} UTC  version {}\n", paperloom_common::log::format_unix(paperloom_common::log::now_unix()), env!("CARGO_PKG_VERSION")));
    out.push_str(&format!("{passed} passed, {} failed, {} s\n", report.failures.len(), started.elapsed().as_secs()));
    if report.failures.is_empty() {
        out.push_str("No step failed.\n");
    } else {
        out.push_str("\nFAILED STEPS\n");
        for failure in &report.failures {
            out.push_str(&format!("  - {failure}\n"));
        }
    }
    out.push_str("\nSTEPS\n");
    for line in &report.lines {
        out.push_str(line);
        out.push('\n');
    }
    if let Some(engine) = engine_output {
        out.push_str("\nENGINE SELF-TEST OUTPUT\n");
        out.push_str(&engine.unwrap_or_else(|err| err));
        out.push('\n');
    }
    let mpv_log = paperloom_common::log::tail("mpv", 60);
    if !mpv_log.is_empty() {
        out.push_str("\nLIBMPV LOG (last 60 lines)\n");
        out.push_str(&mpv_log);
        out.push('\n');
    }
    out.push_str("\n------ DIAGNOSTICS ------\n");
    out.push_str(&paperloom_common::diagnostics::gather(&crate::diagnostics_extra()));
    out
}

/// Runs `paperloom-engine --selftest <clip>` and returns what it printed.
fn engine_selftest(clip: &std::path::Path) -> Result<String, String> {
    let engine = paperloom_common::process::sibling_binary("paperloom-engine").ok_or("paperloom-engine was not found beside paperloom-gui")?;
    let mut command = std::process::Command::new(engine);
    paperloom_common::process::hide_console(&mut command);
    let mut child = command.arg("--selftest").arg(clip).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn().map_err(|err| format!("could not start the engine: {err}"))?;
    let stdout = child.stdout.take().ok_or("no output pipe")?;
    // Read on a thread so a hang can be cut off.
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::Read;
        let mut text = String::new();
        let _ = std::io::BufReader::new(stdout).read_to_string(&mut text);
        let _ = sender.send(text);
    });
    match receiver.recv_timeout(Duration::from_secs(120)) {
        Ok(text) if text.trim().is_empty() => {
            let code = child.wait().ok().and_then(|status| status.code());
            Err(format!("the engine printed nothing and exited with code {}", code.map_or("?".into(), |code| format!("0x{:08X}", code as u32))))
        }
        Ok(text) => {
            let _ = child.wait();
            Ok(text)
        }
        Err(_) => {
            let _ = child.kill();
            Err("the engine self-test did not finish within 2 minutes (it hung)".into())
        }
    }
}

/// Start the real engine, talk to it, play the clip, pause, resume — then put
/// back whatever was playing before.
fn engine_roundtrip(report: &mut Report, clip: &std::path::Path) {
    use paperloom_common::ipc::{self, Command};

    let before = EngineState::load().current_wallpaper;
    let _ = std::fs::remove_file(paperloom_common::engine_log());

    report.step("the wallpaper engine starts and answers", || {
        crate::engine_client::start().map_err(|err| err.to_string())?;
        let started = Instant::now();
        ipc::send(&Command::Ping).map_err(|err| format!("no answer: {err}"))?;
        Ok(format!("answered in {} ms", started.elapsed().as_millis().max(1)))
    });
    report.step("a video plays as the wallpaper", || {
        crate::engine_client::play(clip).map_err(|err| err.to_string())?;
        let started = Instant::now();
        loop {
            std::thread::sleep(Duration::from_millis(500));
            let state = EngineState::load();
            if state.current_wallpaper.as_deref() == Some(clip) && !state.engine.codec.is_empty() {
                let info = state.engine;
                return Ok(format!("{} {}x{}, decoder {}, output {}", info.codec, info.width, info.height, if info.hwdec.is_empty() || info.hwdec == "no" { "software" } else { info.hwdec.as_str() }, info.video_output));
            }
            if started.elapsed() > Duration::from_secs(12) {
                let log = std::fs::read_to_string(paperloom_common::engine_log()).unwrap_or_default();
                return Err(format!("the engine did not report playing the clip within 12 s{}", if log.trim().is_empty() { String::new() } else { format!(" — engine says: {}", log.trim()) }));
            }
        }
    });
    feature_checks(report, &paperloom_common::load());
    report.step("pause and resume", || {
        ipc::send(&Command::Pause).map_err(|err| err.to_string())?;
        std::thread::sleep(Duration::from_secs(3));
        let paused = EngineState::load().user_paused;
        ipc::send(&Command::Resume).map_err(|err| err.to_string())?;
        std::thread::sleep(Duration::from_secs(3));
        let resumed = !EngineState::load().user_paused;
        (paused && resumed).then(|| "paused, then resumed".to_string()).ok_or_else(|| format!("pause reported {paused}, resume reported {resumed}"))
    });

    // Put things back the way they were.
    match before.filter(|path| path.is_file()) {
        Some(path) => {
            let _ = crate::engine_client::play(&path);
            report.info(format!("restored your wallpaper: {}", path.display()));
        }
        None => {
            let _ = ipc::send(&Command::Stop);
            report.info("no wallpaper was playing before; stopped the test clip".into());
        }
    }
}

/// Whether the optional features the settings switch on really came up in the
/// running engine: the panel icon, the global shortcuts, the lock screen still.
fn feature_checks(report: &mut Report, settings: &Settings) {
    // The engine reports within a couple of seconds of starting.
    let wait_for = |pick: &dyn Fn(&paperloom_common::EngineInfo) -> String, seconds: u64| -> String {
        let started = Instant::now();
        loop {
            let value = pick(&EngineState::load().engine);
            if !value.is_empty() && value != "off" || started.elapsed() > Duration::from_secs(seconds) {
                return value;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    };

    if settings.show_tray_icon {
        let status = wait_for(&|info| info.tray.clone(), 8);
        report.step("the tray icon is showing", || match status.as_str() {
            "ok" => Ok("registered with the system".into()),
            "" | "off" => Err("the setting is on but the engine reported nothing".into()),
            other => Err(other.trim_start_matches("failed: ").to_string()),
        });
    } else {
        report.info("tray icon: switched off in Settings".into());
    }

    for (label, text, pick) in [
        ("next-wallpaper shortcut", settings.hotkey_next.clone(), (|info: &paperloom_common::EngineInfo| info.hotkey_next.clone()) as fn(&paperloom_common::EngineInfo) -> String),
        ("pause shortcut", settings.hotkey_pause.clone(), |info: &paperloom_common::EngineInfo| info.hotkey_pause.clone()),
    ] {
        if text.trim().is_empty() {
            continue;
        }
        let status = wait_for(&|info| pick(info), 8);
        report.step(&format!("the {label} ({text}) is registered"), || match status.as_str() {
            "ok" => Ok("active".into()),
            "in use" => Err("another program already uses that key".into()),
            _ => Err("this desktop doesn't support global shortcuts (Wayland and macOS) or the engine reported nothing".into()),
        });
    }

    if settings.lock_screen_still {
        let status = wait_for(&|info| info.lock_screen.clone(), 20);
        report.step("a still of the wallpaper is set as the lock screen picture", || match status.as_str() {
            s if s.starts_with("ok") => Ok(s.to_string()),
            other => Err(other.trim_start_matches("failed: ").to_string()),
        });
    } else {
        report.info("lock screen still: switched off in Settings".into());
    }
}
