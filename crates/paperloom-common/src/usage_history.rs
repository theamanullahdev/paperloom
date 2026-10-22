//! The last five minutes of the wallpaper engine's own CPU and memory use,
//! kept by the engine (which runs whenever a wallpaper plays) so the Usage
//! page already has a line to draw when Paperloom's window opens.
//!
//! A text file of at most 30 lines (one per 10 seconds), rewritten in one go
//! every 10 seconds: a few hundred bytes.

use std::fs;
use std::time::{Duration, SystemTime};

use crate::paths;

pub const KEEP: usize = 30;
pub const EVERY: Duration = Duration::from_secs(10);

/// One reading: percent of the whole CPU, and resident memory in bytes.
pub type Sample = (f32, u64);

pub fn save(samples: &[Sample]) {
    let path = paths::usage_history_file();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let text: String = samples.iter().map(|(cpu, memory)| format!("{cpu:.3} {memory}\n")).collect();
    let temp = path.with_extension("tmp");
    if fs::write(&temp, text).is_ok() {
        let _ = fs::rename(&temp, &path);
    }
}

/// The saved readings, oldest first — only if they are recent (the engine
/// wrote them within the last few minutes).
pub fn load() -> Vec<Sample> {
    let path = paths::usage_history_file();
    let fresh = fs::metadata(&path).and_then(|meta| meta.modified()).ok().and_then(|time| SystemTime::now().duration_since(time).ok()).is_some_and(|age| age < Duration::from_secs(5 * 60));
    if !fresh {
        return Vec::new();
    }
    let text = fs::read_to_string(path).unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            let (cpu, memory) = line.split_once(' ')?;
            Some((cpu.parse().ok()?, memory.parse().ok()?))
        })
        .collect()
}
