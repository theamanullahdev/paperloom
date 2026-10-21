//! The blocking work behind the Developers page's buttons. Everything here
//! runs on a worker thread and returns a line of text to show.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use paperloom_common::ipc::{self, Command};
use paperloom_common::{EngineState, Settings};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Folder {
    Data,
    Config,
    Logs,
    Updates,
    Library,
}

impl Folder {
    pub fn path(self) -> PathBuf {
        match self {
            Folder::Data => paperloom_common::state_file().parent().map(PathBuf::from).unwrap_or_default(),
            Folder::Config => paperloom_common::config_file().parent().map(PathBuf::from).unwrap_or_default(),
            Folder::Logs => paperloom_common::logs_dir(),
            Folder::Updates => paperloom_common::updates_dir(),
            Folder::Library => paperloom_common::wallpapers_dir(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    StartEngine,
    QuitEngine,
    RestartEngine,
    Ping,
    PlayTestClip,
    StopPlayback,
    Pause,
    Resume,
    Mute,
    Unmute,
    DrillEngineFailure,
    DrillPanic,
    ClearStartFailure,
    ClearOnlineCache,
    ClearThumbnails,
    DeleteUpdateFiles,
    ConnectionCheck,
    Benchmark,
    ResetSettings,
}

/// Runs one action and says what happened.
pub fn run(action: Action, settings: &Settings) -> String {
    match action {
        Action::StartEngine => match crate::engine_client::start() {
            Ok(()) => "Engine started".into(),
            Err(err) => format!("Couldn't start the engine: {err}"),
        },
        Action::QuitEngine => match ipc::send(&Command::Quit) {
            Ok(()) => "Sent QUIT".into(),
            Err(err) => format!("No engine answered: {err}"),
        },
        Action::RestartEngine => {
            crate::engine_client::restart_engine();
            "Engine restarted".into()
        }
        Action::Ping => {
            let started = Instant::now();
            match ipc::send(&Command::Ping) {
                Ok(()) => format!("Engine answered in {} ms", started.elapsed().as_millis().max(1)),
                Err(err) => format!("No engine answered: {err}"),
            }
        }
        Action::PlayTestClip => match make_test_clip() {
            Ok(path) => match crate::engine_client::play(&path) {
                Ok(()) => format!("Playing the test clip ({})", path.display()),
                Err(err) => format!("Test clip made, but playback failed: {err}"),
            },
            Err(err) => err,
        },
        Action::StopPlayback => send(Command::Stop, "Sent STOP"),
        Action::Pause => send(Command::Pause, "Sent PAUSE"),
        Action::Resume => send(Command::Resume, "Sent RESUME"),
        Action::Mute => send(Command::Mute(true), "Sent MUTE"),
        Action::Unmute => send(Command::Mute(false), "Sent UNMUTE"),
        Action::DrillEngineFailure => {
            let log = paperloom_common::engine_log();
            if let Some(dir) = log.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(log, "drill: pretend the engine failed to start\n");
            "Wrote a fake engine start failure: the next Set as wallpaper shows it. Clear it with Reset start failure.".into()
        }
        Action::ClearStartFailure => {
            let _ = std::fs::remove_file(paperloom_common::engine_log());
            "Cleared the engine start failure note".into()
        }
        Action::DrillPanic => {
            let _ = std::thread::spawn(|| panic!("developer drill: deliberate panic")).join();
            "Panicked on purpose in a worker thread; see Logs → Crash".into()
        }
        Action::ClearOnlineCache => format!("Removed {} cached preview file(s)", clear_dir(&paperloom_common::online_cache_dir())),
        Action::ClearThumbnails => format!("Removed {} thumbnail file(s); previews are rebuilt on demand", clear_dir(&paperloom_common::thumbnails_dir())),
        Action::DeleteUpdateFiles => format!("Removed {} update file(s)", clear_dir(&paperloom_common::updates_dir())),
        Action::ConnectionCheck => {
            let hosts = crate::pages::network::hosts_for(settings);
            let diagnosis = paperloom_fetch::diagnose(&hosts);
            let mut text = format!("Verdict: {:?}; internet {} ms", diagnosis.verdict, diagnosis.internet_ms.map_or("-".into(), |ms| ms.to_string()));
            for host in diagnosis.hosts {
                text.push_str(&format!(
                    "\n  {}: dns {} ms, connect {} ms",
                    host.host,
                    host.dns_ms.map_or("failed".into(), |ms| ms.to_string()),
                    host.connect_ms.map_or("failed".into(), |ms| ms.to_string())
                ));
            }
            text
        }
        Action::Benchmark => benchmark(Duration::from_secs(20)),
        Action::ResetSettings => {
            let _ = paperloom_common::save(&Settings::default());
            "Settings reset to defaults (restart the app to see every page refresh)".into()
        }
    }
}

fn send(command: Command, done: &str) -> String {
    match ipc::send(&command) {
        Ok(()) => done.to_string(),
        Err(err) => format!("No engine answered: {err}"),
    }
}

fn clear_dir(dir: &std::path::Path) -> usize {
    let mut removed = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && std::fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

/// A five-second moving test pattern, made with the bundled ffmpeg.
fn make_test_clip() -> Result<PathBuf, String> {
    let path = paperloom_common::online_cache_dir().join("paperloom-test-clip.mp4");
    if path.is_file() {
        return Ok(path);
    }
    std::fs::create_dir_all(paperloom_common::online_cache_dir()).map_err(|err| err.to_string())?;
    let output = paperloom_common::process::command("ffmpeg")
        .args(["-y", "-f", "lavfi", "-i", "testsrc2=size=1280x720:rate=30", "-t", "5", "-pix_fmt", "yuv420p"])
        .arg(&path)
        .output()
        .map_err(|err| format!("ffmpeg could not start: {err}"))?;
    if output.status.success() {
        Ok(path)
    } else {
        Err(format!("ffmpeg failed: {}", String::from_utf8_lossy(&output.stderr).lines().last().unwrap_or("no message")))
    }
}

/// Samples the running engine once a second for `length` and summarises it.
fn benchmark(length: Duration) -> String {
    let info = EngineState::load().engine;
    if info.pid == 0 {
        return "No engine is running. Set a wallpaper first.".into();
    }
    let pid = Pid::from_u32(info.pid);
    let mut system = System::new();
    let kind = ProcessRefreshKind::nothing().with_cpu().with_memory();
    system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, kind);
    let cores = std::thread::available_parallelism().map_or(1.0, |n| n.get() as f32);

    let (mut cpu, mut peak, mut memory, mut samples) = (0.0f32, 0.0f32, 0u64, 0u32);
    let started = Instant::now();
    while started.elapsed() < length {
        std::thread::sleep(Duration::from_secs(1));
        system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, kind);
        let Some(process) = system.process(pid) else { return "The engine stopped during the measurement.".into() };
        cpu += process.cpu_usage();
        peak = peak.max(process.cpu_usage());
        memory = memory.max(process.memory());
        samples += 1;
    }
    let state = EngineState::load().engine;
    let average = cpu / samples.max(1) as f32;
    format!(
        "{}s on engine {}: CPU avg {:.1}% of one core ({:.2}% of the whole CPU), peak {:.1}%, memory {:.0} MB\ndecoder: {}, codec {}, {}x{} at {:.0} fps, output {}, dropped {}",
        length.as_secs(),
        info.pid,
        average,
        average / cores,
        peak,
        memory as f64 / 1_048_576.0,
        if state.hwdec.is_empty() || state.hwdec == "no" { "software" } else { state.hwdec.as_str() },
        state.codec,
        state.width,
        state.height,
        state.fps,
        state.video_output,
        state.dropped_frames,
    )
}
