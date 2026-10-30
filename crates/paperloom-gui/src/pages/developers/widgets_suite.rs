//! "Run the full widgets test": exercises the whole desktop-widgets system on
//! this computer and writes one report. It takes the user's widgets off for
//! the time it runs (their settings are kept and put back afterwards), stops
//! and starts the real widgets process several times, and checks, step by
//! step, that the widgets are really on the screen, survive changes, stacks,
//! switching off and on, and don't use much of the computer.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use paperloom_common::dev::DevSwitches;
use paperloom_common::widgets::{self, Status, Widgets, BUILT_IN};

pub use super::suite::Progress;

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
        self.progress.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).pop();
        self.push(line);
    }

    fn info(&mut self, line: String) {
        self.push(format!("[INFO] {line}"));
    }
}

fn wait_until(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < limit {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    done()
}

/// Shows the desktop (minimises every window), or brings the windows back (Windows).
fn desktop(on: bool) {
    if let Some(program) = paperloom_common::process::sibling_binary("paperloom-widgets") {
        let mut command = std::process::Command::new(program);
        paperloom_common::process::hide_console(&mut command);
        let _ = command.args(["--show-desktop", if on { "on" } else { "off" }]).stdin(std::process::Stdio::null()).output();
    }
}

fn screen() -> (u32, u32) {
    paperloom_common::primary_size().unwrap_or((1920, 1080))
}

/// One widget of every kind, spread over the screen.
fn every_kind() -> Widgets {
    let mut all = Widgets { enabled: true, placed: Vec::new(), ..Widgets::default() };
    for (index, entry) in BUILT_IN.iter().enumerate() {
        if let Some(id) = all.add(entry.kind) {
            if let Some(placed) = all.placed.iter_mut().find(|placed| placed.id == id) {
                placed.x = 0.03 + (index % 5) as f32 * 0.19;
                placed.y = 0.05 + (index / 5) as f32 * 0.3;
            }
        }
    }
    all
}

/// Saves `all` and waits for the process to show every window, then returns its status.
fn show(all: &Widgets, wait: Duration) -> Result<Status, String> {
    all.save().map_err(|err| format!("couldn't save the widget settings: {err}"))?;
    widgets::start().map_err(|err| format!("couldn't start the widgets process: {err}"))?;
    if !wait_until(Duration::from_secs(15), widgets::is_running) {
        return Err(format!("the widgets process did not start ({})", log_tail()));
    }
    let wanted = all.placed.len() as u32;
    wait_until(wait, || Status::read().is_some_and(|status| status.windows == wanted && (status.verified || !cfg!(windows)) && status.paused.is_empty()));
    Status::read().ok_or_else(|| "the widgets process wrote no status".to_string())
}

fn describe(status: &Status) -> String {
    format!(
        "draws on: {} · {} windows · seen on screen: {} · hidden: {} · problem: {}",
        if status.backend.is_empty() { "—" } else { &status.backend },
        status.windows,
        if status.verified { "yes" } else { "no" },
        if status.paused.is_empty() { "no" } else { &status.paused },
        if status.problem.is_empty() { "none" } else { &status.problem }
    )
}

/// Checks a status says the widgets are really there.
fn judge(status: &Status, wanted: usize) -> Result<String, String> {
    let line = describe(status);
    if !status.problem.is_empty() {
        return Err(line);
    }
    if status.windows as usize != wanted {
        return Err(format!("{wanted} windows were expected — {line}"));
    }
    if cfg!(windows) && !status.verified {
        return Err(format!("the windows exist but nothing was seen on the screen — {line}"));
    }
    Ok(line)
}

fn log_path() -> PathBuf {
    paperloom_common::logs_dir().join("widgets.log")
}

fn log_len() -> u64 {
    std::fs::metadata(log_path()).map(|meta| meta.len()).unwrap_or(0)
}

fn log_tail() -> String {
    paperloom_common::log::tail("widgets", 3).replace('\n', " | ")
}

/// What was written to a file after `from` bytes.
fn appended(path: &std::path::Path, from: u64) -> String {
    std::fs::read(path).map(|bytes| String::from_utf8_lossy(bytes.get(from as usize..).unwrap_or_default()).into_owned()).unwrap_or_default()
}

pub fn run(progress: Progress) -> String {
    progress.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clear();
    let mut report = Report { lines: Vec::new(), failures: Vec::new(), progress };
    let started = Instant::now();
    let (log_from, crash_from) = (log_len(), std::fs::metadata(paperloom_common::logs_dir().join("crash.log")).map(|meta| meta.len()).unwrap_or(0));

    // Everything the user has is kept and put back at the end.
    let saved_settings = std::fs::read_to_string(paperloom_common::widgets_file()).ok();
    let was_running = widgets::is_running();
    let switches = DevSwitches::load();
    let mut for_test = switches.clone();
    // The test is about the widgets, not the wallpaper's pause rules.
    for_test.disable_auto_pause = true;
    let _ = for_test.save();
    report.info(format!("the widgets process was {}; your widget settings are kept and restored afterwards", if was_running { "running" } else { "not running" }));
    crate::engine_client::end_widgets();
    let _ = std::fs::remove_file(paperloom_common::widgets_status());

    report.step("the widgets program is next to the app", || {
        paperloom_common::process::sibling_binary("paperloom-widgets").map(|path| path.display().to_string()).ok_or_else(|| "paperloom-widgets was not found beside paperloom-gui — reinstall Paperloom".to_string())
    });

    report.step("drawing self-test (every look of every kind, in memory)", || {
        let output = super::actions::widgets_selftest()?;
        let failed: Vec<&str> = output.lines().filter(|line| line.starts_with("[FAIL]")).collect();
        if failed.is_empty() {
            Ok(format!("{} checks passed", output.lines().filter(|line| line.starts_with("[PASS]")).count()))
        } else {
            Err(failed.join(" | "))
        }
    });

    report.step("real desktop test (every way of showing, with a test patch looked for on the screen)", || {
        let output = super::actions::widgets_probe()?;
        let detail = output.lines().map(|line| line.trim_start_matches("[PASS] ").trim_start_matches("[FAIL] ").trim_start_matches("[INFO] ")).collect::<Vec<_>>().join(" · ");
        if output.lines().any(|line| line.starts_with("[FAIL] widgets can NOT")) {
            Err(detail)
        } else {
            Ok(detail)
        }
    });

    // --- One of every kind, for real. The desktop is shown (other windows minimised, this one
    // too) so it can be looked at, and everything is brought back at the end.
    if cfg!(windows) {
        report.info("minimising all windows for the test, so the desktop can be looked at (they come back at the end)".to_string());
        desktop(true);
        std::thread::sleep(Duration::from_secs(1));
    }
    let all = every_kind();
    let wanted = all.placed.len();
    let mut shown: Option<Status> = None;
    report.step(&format!("one widget of every kind ({wanted}) starts and is seen on the screen"), || {
        let status = show(&all, Duration::from_secs(25))?;
        let verdict = judge(&status, wanted);
        shown = Some(status);
        verdict
    });
    let first_pid = shown.as_ref().map_or(0, |status| status.pid);

    report.step("the widgets process keeps running", || {
        std::thread::sleep(Duration::from_secs(4));
        if widgets::is_running() { Ok("still running after 4 seconds".to_string()) } else { Err(format!("it stopped by itself ({})", log_tail())) }
    });

    report.step("it uses little of the computer (6 seconds measured)", || {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
        let pid = Pid::from_u32(Status::read().map_or(0, |status| status.pid));
        let mut system = System::new();
        let kind = ProcessRefreshKind::nothing().with_cpu().with_memory();
        system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, kind);
        let mut samples = Vec::new();
        for _ in 0..6 {
            std::thread::sleep(Duration::from_secs(1));
            system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, kind);
            let process = system.process(pid).ok_or("the process is gone")?;
            samples.push((process.cpu_usage(), process.memory()));
        }
        let cpu = samples.iter().map(|sample| sample.0).sum::<f32>() / samples.len() as f32;
        let memory = samples.iter().map(|sample| sample.1).max().unwrap_or(0) / 1_048_576;
        let line = format!("{cpu:.1}% of a processor core on average, {memory} MB of memory");
        if cpu > 25.0 || memory > 300 {
            Err(line)
        } else {
            Ok(line)
        }
    });

    report.step("it keeps going when a setting changes (the old version quit here)", || {
        let mut changed = all.clone();
        if let Some(first) = changed.placed.first_mut() {
            first.scale = 1.3;
        }
        changed.save().map_err(|err| err.to_string())?;
        std::thread::sleep(Duration::from_secs(6));
        if !widgets::is_running() {
            return Err(format!("it stopped after a setting changed ({})", log_tail()));
        }
        let status = Status::read().ok_or("no status")?;
        if first_pid != 0 && status.pid != first_pid {
            return Err(format!("it restarted (pid {first_pid} → {})", status.pid));
        }
        judge(&status, wanted)
    });

    report.step("removing and adding widgets while it runs", || {
        let mut fewer = all.clone();
        fewer.placed.truncate(wanted / 2);
        fewer.save().map_err(|err| err.to_string())?;
        wait_until(Duration::from_secs(12), || Status::read().is_some_and(|status| status.windows as usize == fewer.placed.len()));
        let after_remove = Status::read().ok_or("no status")?;
        if after_remove.windows as usize != fewer.placed.len() {
            return Err(format!("{} windows were expected after removing — {}", fewer.placed.len(), describe(&after_remove)));
        }
        all.save().map_err(|err| err.to_string())?;
        wait_until(Duration::from_secs(12), || Status::read().is_some_and(|status| status.windows as usize == wanted));
        let status = Status::read().ok_or("no status")?;
        if !widgets::is_running() {
            return Err("it stopped".to_string());
        }
        judge(&status, wanted)
    });

    report.step("stacks: every preset on the desktop at once", || {
        let mut stacked = Widgets { enabled: true, placed: Vec::new(), ..Widgets::default() };
        let mut added = Vec::new();
        for preset in paperloom_common::stacks::presets() {
            if stacked.add_stack(&preset, screen()).is_some() {
                added.push(preset.name.clone());
            }
        }
        if added.is_empty() {
            return Err("no preset could be placed".to_string());
        }
        stacked.save().map_err(|err| err.to_string())?;
        let count = stacked.placed.len();
        wait_until(Duration::from_secs(15), || Status::read().is_some_and(|status| status.windows as usize == count));
        let status = Status::read().ok_or("no status")?;
        let line = judge(&status, count)?;
        // The stack logic itself: moving one widget moves the whole stack; ungrouping keeps them.
        let first = stacked.placed.first().map(|placed| placed.id).ok_or("no widgets")?;
        let stack = stacked.placed[0].stack;
        let before: Vec<f32> = stacked.members(stack).map(|placed| placed.x).collect();
        stacked.move_member(first, stacked.placed[0].x + 0.05, stacked.placed[0].y, screen());
        let after: Vec<f32> = stacked.members(stack).map(|placed| placed.x).collect();
        if before.len() > 1 && before.iter().zip(&after).any(|(a, b)| (b - a).abs() < 1e-4) && stacked.stack(stack).is_some_and(|s| s.x < 0.9) {
            return Err("moving one widget of a stack did not move the others".to_string());
        }
        stacked.ungroup(stack);
        stacked.remove_stack(stacked.stacks.first().map_or(0, |stack| stack.id));
        Ok(format!("{} stacks ({}) placed; moving, ungrouping and removing work — {line}", added.len(), added.join(", ")))
    });

    report.step("switching widgets off ends the process, switching on starts it", || {
        let mut off = all.clone();
        off.enabled = false;
        off.save().map_err(|err| err.to_string())?;
        if !wait_until(Duration::from_secs(10), || !widgets::is_running()) {
            return Err("it kept running after widgets were switched off".to_string());
        }
        widgets::set_enabled(true).map_err(|err| format!("couldn't switch on: {err}"))?;
        if !wait_until(Duration::from_secs(15), widgets::is_running) {
            return Err("it did not start after widgets were switched on".to_string());
        }
        Ok("off ended it, on started it again".to_string())
    });

    report.step("the weather service answers", || {
        let output = paperloom_common::process::command("curl").args(["-sS", "-m", "12", "https://geocoding-api.open-meteo.com/v1/search?name=London&count=1"]).output().map_err(|err| format!("curl couldn't run: {err}"))?;
        let body = String::from_utf8_lossy(&output.stdout);
        if body.contains("\"results\"") { Ok("Open-Meteo answered".to_string()) } else { Err(format!("no answer ({})", String::from_utf8_lossy(&output.stderr).lines().next().unwrap_or("offline?"))) }
    });

    report.step("the widgets log has no errors from this run", || {
        let text = appended(&log_path(), log_from);
        let errors: Vec<&str> = text.lines().filter(|line| line.contains(" ERROR ") || line.contains("panic")).collect();
        let warnings = text.lines().filter(|line| line.contains(" WARN ")).count();
        if errors.is_empty() { Ok(format!("{warnings} warnings, no errors")) } else { Err(errors.join(" | ")) }
    });
    report.step("no crash was recorded", || {
        let text = appended(&paperloom_common::logs_dir().join("crash.log"), crash_from);
        if text.trim().is_empty() { Ok("crash log unchanged".to_string()) } else { Err(text.lines().take(3).collect::<Vec<_>>().join(" | ")) }
    });

    // --- Put everything back
    if cfg!(windows) {
        desktop(false);
    }
    crate::engine_client::end_widgets();
    let _ = switches.save();
    match &saved_settings {
        Some(text) => {
            let _ = std::fs::write(paperloom_common::widgets_file(), text);
        }
        None => {
            let _ = std::fs::remove_file(paperloom_common::widgets_file());
        }
    }
    let restored = Widgets::load();
    if restored.enabled && !restored.placed.is_empty() && (was_running || saved_settings.is_some()) {
        let _ = widgets::start();
    }
    report.info(format!("your widgets are back as they were ({} placed, {})", restored.placed.len(), if restored.enabled { "on" } else { "off" }));

    let mut out = String::new();
    out.push_str(&format!("Paperloom {} widgets test — {} — {}\n", env!("CARGO_PKG_VERSION"), paperloom_common::log::format_unix(paperloom_common::log::now_unix()), std::env::consts::OS));
    out.push_str(&if report.failures.is_empty() {
        format!("RESULT: every check passed ({} s)\n", started.elapsed().as_secs())
    } else {
        format!("RESULT: {} check(s) FAILED ({} s): {}\n", report.failures.len(), started.elapsed().as_secs(), report.failures.iter().map(|failure| failure.split(':').next().unwrap_or("").to_string()).collect::<Vec<_>>().join("; "))
    });
    out.push('\n');
    for line in &report.lines {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("\n== Widgets log (this run) ==\n");
    out.push_str(appended(&log_path(), log_from).trim());
    out.push_str("\n\n== Status the process reported last ==\n");
    out.push_str(&Status::read().map(|status| describe(&status)).unwrap_or_else(|| "(none)".to_string()));
    out.push('\n');
    out
}
