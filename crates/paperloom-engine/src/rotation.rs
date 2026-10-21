//! Runs the wallpaper rotation and schedule (`paperloom_common::rotation`).
//!
//! Called on the engine's ~2 s tick. Between changes it costs one `stat` of
//! `rotation.toml` and a clock read; the library is read only when a change
//! is actually due.

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use libmpv2::Mpv;
use paperloom_common::rotation::{local_now, Rotation};
use paperloom_common::{EngineState, Library};

struct State {
    collection: Option<u64>,
    changed: Instant,
    file_time: Option<SystemTime>,
    rotation: Rotation,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

/// Applies the rotation if a change is due.
pub fn tick(mpv: &Mpv) {
    let mut slot = STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let state = slot.get_or_insert_with(|| State { collection: None, changed: Instant::now(), file_time: None, rotation: Rotation::default() });

    let file_time = std::fs::metadata(paperloom_common::paths_rotation_file()).and_then(|meta| meta.modified()).ok();
    if file_time != state.file_time {
        state.file_time = file_time;
        state.rotation = Rotation::load();
    }

    let Some(wanted) = state.rotation.wanted(local_now()) else {
        state.collection = None;
        return;
    };
    let switched = state.collection != Some(wanted);
    let interval = Duration::from_secs(u64::from(state.rotation.interval_minutes) * 60);
    let due = switched || (state.rotation.interval_minutes > 0 && state.changed.elapsed() >= interval);
    if !due {
        return;
    }
    state.collection = Some(wanted);
    state.changed = Instant::now();
    // Turning a collection on keeps the current wallpaper if it already belongs to it.
    change(mpv, wanted, state.rotation.shuffle, switched);
}

/// The IPC `Next`: moves on right now.
pub fn next(mpv: &Mpv) {
    let mut slot = STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let rotation = Rotation::load();
    let Some(collection) = rotation.wanted(local_now()).or(rotation.collection) else { return };
    if let Some(state) = slot.as_mut() {
        state.collection = Some(collection);
        state.changed = Instant::now();
    }
    change(mpv, collection, rotation.shuffle, false);
}

fn change(mpv: &Mpv, collection: u64, shuffle: bool, keep_if_member: bool) {
    let library = Library::load_manifest();
    let Some(collection) = library.collection(collection) else { return };
    let playable: Vec<_> = collection.members.iter().filter_map(|id| library.entry(*id)).filter(|entry| !entry.hidden && entry.path.is_file()).map(|entry| entry.path.clone()).collect();
    if playable.is_empty() {
        return;
    }
    let current = EngineState::load().current_wallpaper;
    if keep_if_member && current.as_ref().is_some_and(|path| playable.contains(path)) {
        return;
    }
    let position = current.as_ref().and_then(|path| playable.iter().position(|candidate| candidate == path));
    let index = match (shuffle && playable.len() > 1, position) {
        (true, _) => random_other(playable.len(), position),
        (false, Some(at)) => (at + 1) % playable.len(),
        (false, None) => 0,
    };
    crate::player::play(mpv, &playable[index].to_string_lossy());
}

/// A pseudo-random index different from `avoid` (clock-based; no need for
/// real randomness to shuffle wallpapers).
fn random_other(len: usize, avoid: Option<usize>) -> usize {
    let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |elapsed| elapsed.subsec_nanos() as usize);
    let mut index = nanos % len;
    if Some(index) == avoid {
        index = (index + 1 + nanos / 7 % (len - 1)) % len;
    }
    index
}
