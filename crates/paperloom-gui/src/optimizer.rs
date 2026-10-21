//! The background step after an import in "Copy & optimize" mode: look at
//! the copy and, only if it would be cheaper to play, re-encode it (see
//! `paperloom_common::media` for the policy).
//!
//! One job at a time, on a low-priority ffmpeg, and not while a laptop runs
//! on battery. The worker thread starts with the first job and sleeps on a
//! channel between jobs, so an app that imports nothing runs nothing. A
//! file is replaced only after the new one checks out (same length, plays);
//! otherwise the original stays.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use paperloom_common::media;

type Outcome = Result<Option<PathBuf>, String>;

#[derive(Default)]
struct Shared {
    queue: VecDeque<(u64, PathBuf)>,
    working: Option<u64>,
    percent: HashMap<u64, u8>,
    finished: Vec<(u64, Outcome)>,
}

#[derive(Clone, Default)]
pub struct Optimizer {
    shared: Arc<(Mutex<Shared>, Condvar)>,
    started: Arc<Mutex<bool>>,
}

impl Optimizer {
    pub fn enqueue(&self, id: u64, path: PathBuf) {
        {
            let mut state = self.shared.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.working == Some(id) || state.queue.iter().any(|(queued, _)| *queued == id) {
                return;
            }
            state.queue.push_back((id, path));
        }
        self.shared.1.notify_one();
        let mut started = self.started.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if !*started {
            *started = true;
            let shared = self.shared.clone();
            let _ = std::thread::Builder::new().name("paperloom-optimizer".into()).spawn(move || worker(shared));
        }
    }

    /// Anything queued or running (the app ticks while this is true).
    pub fn busy(&self) -> bool {
        let state = self.shared.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.working.is_some() || !state.queue.is_empty()
    }

    /// 0–100 while this wallpaper is being re-encoded; `Some(0)` while it waits.
    pub fn progress(&self, id: u64) -> Option<u8> {
        let state = self.shared.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.working == Some(id) {
            Some(state.percent.get(&id).copied().unwrap_or(0))
        } else {
            state.queue.iter().any(|(queued, _)| *queued == id).then_some(0)
        }
    }

    pub fn take_finished(&self) -> Vec<(u64, Outcome)> {
        std::mem::take(&mut self.shared.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).finished)
    }
}

fn worker(shared: Arc<(Mutex<Shared>, Condvar)>) {
    loop {
        let (id, path) = {
            let (lock, wake) = &*shared;
            let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            while state.queue.is_empty() {
                state = wake.wait(state).unwrap_or_else(|poisoned| poisoned.into_inner());
            }
            let job = state.queue.pop_front().expect("queue is not empty");
            state.working = Some(job.0);
            job
        };

        // A laptop on battery waits for the charger.
        while on_battery() {
            std::thread::sleep(Duration::from_secs(60));
        }

        let outcome = optimize(&path, |percent| {
            shared.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).percent.insert(id, percent);
        });
        paperloom_common::log::info(&format!("optimize {}: {:?}", path.display(), outcome.as_ref().map(|replaced| replaced.is_some())));

        let mut state = shared.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.working = None;
        state.percent.remove(&id);
        state.finished.push((id, outcome));
    }
}

/// `Ok(None)`: already suitable. `Ok(Some(path))`: replaced by a cheaper copy.
fn optimize(path: &Path, progress: impl Fn(u8)) -> Outcome {
    let Some(info) = media::probe(path) else { return Ok(None) };
    let Some(plan) = media::plan(&info, paperloom_common::primary_size()) else { return Ok(None) };

    // The result is always an .mp4, whatever the original container was.
    let target = path.with_extension("mp4");
    if target != path && target.exists() {
        return Err("a file with the converted name already exists".into());
    }
    let temp = path.with_extension("optimizing.mp4");
    let _ = std::fs::remove_file(&temp);
    let mut command = low_priority_ffmpeg();
    command.args(media::encode_args(&plan, &info, path, &temp)).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = command.spawn().map_err(|err| format!("couldn't start ffmpeg: {err}"))?;

    if let Some(stdout) = child.stdout.take() {
        let total = (info.duration_secs.max(0.1) * 1_000_000.0) as f64;
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(done) = line.strip_prefix("out_time_us=").and_then(|value| value.trim().parse::<f64>().ok()) {
                progress(((done / total) * 100.0).clamp(0.0, 99.0) as u8);
            }
        }
    }
    let status = child.wait().map_err(|err| err.to_string())?;
    if !status.success() {
        let _ = std::fs::remove_file(&temp);
        return Err("ffmpeg failed".into());
    }

    // Trust the new file only if it reads back with the same length and is
    // actually smaller or cheaper (smaller on disk, or fewer pixels/frames).
    let ok = media::probe(&temp).is_some_and(|new| (new.duration_secs - info.duration_secs).abs() < 1.5 && new.codec == "h264");
    let smaller = std::fs::metadata(&temp).map(|new| new.len()).unwrap_or(u64::MAX) <= std::fs::metadata(path).map(|old| old.len()).unwrap_or(0).saturating_add(1_000_000);
    if !ok || !(smaller || plan.scale_to.is_some() || plan.fps_cap.is_some() || plan.change_format) {
        let _ = std::fs::remove_file(&temp);
        return Err("the converted copy didn't check out; the original is kept".into());
    }
    std::fs::rename(&temp, &target).map_err(|err| {
        let _ = std::fs::remove_file(&temp);
        format!("couldn't replace the file (is it playing?): {err}")
    })?;
    if target != path {
        let _ = std::fs::remove_file(path);
    }
    Ok(Some(target))
}

#[cfg(unix)]
fn low_priority_ffmpeg() -> std::process::Command {
    let program = paperloom_common::process::sibling_binary("ffmpeg").unwrap_or_else(|| PathBuf::from("ffmpeg"));
    let mut command = std::process::Command::new("nice");
    command.args(["-n", "19"]).arg(program);
    command
}

#[cfg(windows)]
fn low_priority_ffmpeg() -> std::process::Command {
    use std::os::windows::process::CommandExt;
    const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let program = paperloom_common::process::sibling_binary("ffmpeg").unwrap_or_else(|| PathBuf::from("ffmpeg.exe"));
    let mut command = std::process::Command::new(program);
    command.creation_flags(BELOW_NORMAL_PRIORITY_CLASS | CREATE_NO_WINDOW);
    command
}

/// Linux: any battery reports "Discharging". Elsewhere: assume mains power.
fn on_battery() -> bool {
    #[cfg(target_os = "linux")]
    {
        if let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") {
            return entries.flatten().any(|entry| std::fs::read_to_string(entry.path().join("status")).is_ok_and(|status| status.trim() == "Discharging"));
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ffmpeg_present() -> bool {
        paperloom_common::process::command("ffmpeg").arg("-version").output().is_ok_and(|out| out.status.success())
    }

    /// A 4K 60 fps clip gets scaled down and capped; a plain 720p30 one is left alone.
    #[test]
    fn converts_only_when_worthwhile() {
        if !ffmpeg_present() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("paperloom-opt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let make = |name: &str, size: &str, rate: u32| {
            let path = dir.join(name);
            let ok = paperloom_common::process::command("ffmpeg")
                .args(["-y", "-f", "lavfi", "-i", &format!("testsrc2=size={size}:rate={rate}"), "-t", "2", "-pix_fmt", "yuv420p"])
                .arg(&path)
                .output()
                .is_ok_and(|out| out.status.success());
            assert!(ok, "ffmpeg could not make the test clip");
            path
        };

        let big = make("big.mp4", "3840x2160", 60);
        let replaced = optimize(&big, |_| {}).expect("optimizes");
        if paperloom_common::primary_size().is_some() {
            assert_eq!(replaced.as_deref(), Some(big.as_path())); // already an .mp4
            let info = media::probe(&big).unwrap();
            assert!(info.width < 3840 && info.fps <= 31.0, "{info:?}");
        }

        let small = make("small.mp4", "1280x720", 30);
        assert_eq!(optimize(&small, |_| {}).unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
