//! Persistent download queue with one background worker.
//!
//! The promise: once a wallpaper is queued, the queue always keeps its
//! reference (id, title, URL). Transient failures — network down, timeouts,
//! 429/5xx — retry forever with capped exponential backoff, resuming from
//! the `.part` file; the queue is saved to disk on every state change, so
//! quitting the app or rebooting resumes where it left off. Only a
//! permanent failure (404, not an MP4) stops retrying automatically, and
//! even then the job stays listed with a manual "Retry".
//!
//! Order is strict: the worker always takes the first job in the list that
//! is not finished, failed or paused. If that job is waiting out a network
//! hiccup, everything behind it waits too — a flaky connection never lets a
//! lower job jump the queue. Pause a job to let the ones below it go ahead.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use paperloom_common::downloads_dir;
use serde::{Deserialize, Serialize};

use crate::download::download;
use crate::http::FetchError;
use crate::source::OnlineItem;

const BACKOFF_START: Duration = Duration::from_secs(5);
const BACKOFF_CAP: Duration = Duration::from_secs(120);
const GAP_BETWEEN_JOBS: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Queued,
    Downloading,
    /// Failed transiently; will retry on its own.
    Waiting,
    Done,
    /// Failed permanently; needs a manual retry.
    Failed,
    /// Held by the user; the queue goes on with the others.
    Paused,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub source: String,
    pub title: String,
    pub url: String,
    pub file_name: String,
    /// "HD" or "4K": the rendition the user chose.
    #[serde(default)]
    pub quality: String,
    pub status: Status,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
    pub attempts: u32,
    pub last_error: Option<String>,
    /// Seconds until the next automatic retry, for display. Filled in by
    /// `Queue::snapshot`; never persisted.
    #[serde(skip)]
    pub retry_in: Option<u64>,
}

impl Job {
    pub fn final_path(&self) -> PathBuf {
        downloads_dir().join(&self.file_name)
    }

    pub fn part_path(&self) -> PathBuf {
        downloads_dir().join(format!("{}.part", self.file_name))
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Manifest {
    jobs: Vec<Job>,
}

#[derive(Default)]
struct State {
    jobs: Vec<Job>,
    retry_at: HashMap<String, Instant>,
    active: Option<(String, Arc<AtomicBool>)>,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

/// Cheap to clone; all clones talk to the same worker.
#[derive(Clone)]
pub struct Queue {
    shared: Arc<Shared>,
}

fn manifest_path() -> PathBuf {
    downloads_dir().join("queue.toml")
}

impl Queue {
    /// Loads the saved queue (anything that was mid-download goes back to
    /// `Queued`) and starts the worker thread.
    pub fn start() -> Self {
        let mut jobs: Vec<Job> = std::fs::read_to_string(manifest_path())
            .ok()
            .and_then(|text| toml::from_str::<Manifest>(&text).ok())
            .map(|manifest| manifest.jobs)
            .unwrap_or_default();
        for job in &mut jobs {
            if matches!(job.status, Status::Downloading | Status::Waiting) {
                job.status = Status::Queued;
            }
            // Progress isn't saved on every chunk; the part file is the truth.
            if let Ok(meta) = job.part_path().metadata() {
                job.bytes_done = meta.len();
            }
        }

        let queue = Queue { shared: Arc::new(Shared { state: Mutex::new(State { jobs, ..State::default() }), wake: Condvar::new() }) };
        let worker = queue.clone();
        std::thread::Builder::new().name("paperloom-downloads".into()).spawn(move || worker.run()).expect("spawn download worker");
        queue
    }

    /// Queues `item` in the given rendition (`"HD"` or `"4K"`). Returns
    /// `false` if that wallpaper is already queued or
    /// downloaded-but-not-yet-imported, or has no such rendition.
    pub fn add(&self, item: &OnlineItem, quality: &str) -> bool {
        let Some(variant) = item.variant(quality) else { return false };
        let id = item.key();
        let mut state = self.lock();
        if state.jobs.iter().any(|job| job.id == id) {
            return false;
        }
        state.jobs.push(Job {
            file_name: file_name_for(&item.source, &item.id, &item.title, quality),
            id,
            source: item.source_name.clone(),
            quality: quality.to_string(),
            title: item.title.clone(),
            url: variant.url.clone(),
            status: Status::Queued,
            bytes_done: 0,
            bytes_total: None,
            attempts: 0,
            last_error: None,
            retry_in: None,
        });
        save(&state);
        drop(state);
        self.shared.wake.notify_all();
        true
    }

    /// Jobs in priority order (the worker always takes the first one that
    /// is ready), with `retry_in` filled in.
    pub fn snapshot(&self) -> Vec<Job> {
        let state = self.lock();
        let now = Instant::now();
        state
            .jobs
            .iter()
            .cloned()
            .map(|mut job| {
                job.retry_in = state.retry_at.get(&job.id).map(|at| at.saturating_duration_since(now).as_secs());
                job
            })
            .collect()
    }

    /// Moves a job to `position` in the queue (0 = next to download). A
    /// download already in flight isn't interrupted, but the new order
    /// decides what starts after it.
    pub fn move_job(&self, id: &str, position: usize) {
        let mut state = self.lock();
        let Some(from) = state.jobs.iter().position(|job| job.id == id) else { return };
        let job = state.jobs.remove(from);
        let to = position.min(state.jobs.len());
        state.jobs.insert(to, job);
        save(&state);
        drop(state);
        self.shared.wake.notify_all();
    }

    pub fn is_busy(&self) -> bool {
        self.lock().jobs.iter().any(|job| !matches!(job.status, Status::Failed | Status::Done | Status::Paused))
    }

    /// Holds a job: a download in flight stops (its partial file is kept),
    /// and the queue moves on to the next job.
    pub fn pause(&self, id: &str) {
        let mut state = self.lock();
        if let Some((active_id, flag)) = &state.active {
            if active_id == id {
                flag.store(true, Ordering::Relaxed);
            }
        }
        state.retry_at.remove(id);
        if let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) {
            if matches!(job.status, Status::Queued | Status::Waiting | Status::Downloading) {
                job.status = Status::Paused;
                job.last_error = None;
            }
        }
        save(&state);
        drop(state);
        self.shared.wake.notify_all();
    }

    /// Puts a paused job back in line (in its place).
    pub fn resume(&self, id: &str) {
        let mut state = self.lock();
        if let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) {
            if job.status == Status::Paused {
                job.status = Status::Queued;
            }
        }
        save(&state);
        drop(state);
        self.shared.wake.notify_all();
    }

    pub fn pause_all(&self) {
        let ids: Vec<String> = self.lock().jobs.iter().filter(|job| matches!(job.status, Status::Queued | Status::Waiting | Status::Downloading)).map(|job| job.id.clone()).collect();
        for id in ids {
            self.pause(&id);
        }
    }

    pub fn resume_all(&self) {
        let ids: Vec<String> = self.lock().jobs.iter().filter(|job| job.status == Status::Paused).map(|job| job.id.clone()).collect();
        for id in ids {
            self.resume(&id);
        }
    }

    /// Jobs whose file is complete and waiting for the caller to import it.
    pub fn finished(&self) -> Vec<Job> {
        self.lock().jobs.iter().filter(|job| job.status == Status::Done).cloned().collect()
    }

    /// Removes a finished job once its file has been imported (the
    /// downloaded copy is deleted — the library keeps its own).
    pub fn acknowledge(&self, id: &str, delete_file: bool) {
        let mut state = self.lock();
        if let Some(position) = state.jobs.iter().position(|job| job.id == id) {
            let job = state.jobs.remove(position);
            if delete_file {
                let _ = std::fs::remove_file(job.final_path());
            }
            save(&state);
        }
    }

    /// Immediately retries a waiting or failed job.
    pub fn retry(&self, id: &str) {
        let mut state = self.lock();
        state.retry_at.remove(id);
        if let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) {
            if matches!(job.status, Status::Failed | Status::Waiting) {
                job.status = Status::Queued;
                job.last_error = None;
            }
        }
        save(&state);
        drop(state);
        self.shared.wake.notify_all();
    }

    /// Drops a job and its partial file.
    pub fn cancel(&self, id: &str) {
        let mut state = self.lock();
        if let Some((active_id, flag)) = &state.active {
            if active_id == id {
                flag.store(true, Ordering::Relaxed);
            }
        }
        if let Some(position) = state.jobs.iter().position(|job| job.id == id) {
            let job = state.jobs.remove(position);
            let _ = std::fs::remove_file(job.part_path());
            state.retry_at.remove(id);
            save(&state);
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.shared.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn run(&self) {
        loop {
            let (job, cancel) = self.next_job();

            let result = {
                let progress_queue = self.clone();
                let id = job.id.clone();
                download(job.id.split(':').next().unwrap_or(""), &job.url, &job.part_path(), &job.final_path(), &cancel, move |done, total| {
                    let mut state = progress_queue.lock();
                    if let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) {
                        job.bytes_done = done;
                        job.bytes_total = total;
                    }
                })
            };

            self.finish(&job.id, result);
            std::thread::sleep(GAP_BETWEEN_JOBS);
        }
    }

    /// Blocks until a job is eligible *and* the internet is reachable,
    /// marks it `Downloading`, returns it. While offline, queued jobs just
    /// wait (no attempts burned, no backoff growth).
    fn next_job(&self) -> (Job, Arc<AtomicBool>) {
        loop {
            let state = self.lock();
            let id = match pick_next(&state.jobs, &state.retry_at, Instant::now()) {
                Pick::Start(position) => state.jobs[position].id.clone(),
                Pick::Wait(wait) => {
                    drop(self.shared.wake.wait_timeout(state, wait).unwrap_or_else(|poisoned| poisoned.into_inner()));
                    continue;
                }
            };
            drop(state);

            // Checked without holding the lock: this can take ~1s.
            if !crate::net::is_online() {
                let mut state = self.lock();
                if let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) {
                    job.last_error = Some("Waiting for internet connection".to_string());
                }
                drop(self.shared.wake.wait_timeout(state, Duration::from_secs(3)).unwrap_or_else(|poisoned| poisoned.into_inner()));
                continue;
            }

            let mut state = self.lock();
            // The job may have been cancelled, paused or already started meanwhile.
            let Some(position) = state.jobs.iter().position(|job| job.id == id && matches!(job.status, Status::Queued | Status::Waiting)) else {
                continue;
            };
            let cancel = Arc::new(AtomicBool::new(false));
            state.jobs[position].status = Status::Downloading;
            state.jobs[position].last_error = None;
            state.retry_at.remove(&id);
            let job = state.jobs[position].clone();
            state.active = Some((id, cancel.clone()));
            save(&state);
            return (job, cancel);
        }
    }

    fn finish(&self, id: &str, result: Result<(), FetchError>) {
        let mut state = self.lock();
        state.active = None;
        let mut delay = None;
        if let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) {
            // Paused while it was downloading: the stop is the user's doing,
            // not a failure.
            if job.status == Status::Paused {
                job.bytes_done = job.part_path().metadata().map_or(job.bytes_done, |meta| meta.len());
                save(&state);
                return;
            }
            match result {
                Ok(()) => {
                    job.status = Status::Done;
                    job.last_error = None;
                    if let Ok(meta) = job.final_path().metadata() {
                        job.bytes_done = meta.len();
                        job.bytes_total = Some(meta.len());
                    }
                }
                Err(FetchError::Permanent(message)) => {
                    job.status = Status::Failed;
                    job.last_error = Some(message);
                }
                Err(FetchError::Offline) => {
                    job.status = Status::Queued;
                    job.last_error = Some("Waiting for internet connection".to_string());
                }
                Err(FetchError::Transient(message)) => {
                    job.attempts += 1;
                    job.status = Status::Waiting;
                    job.last_error = Some(message);
                    delay = Some(backoff(job.attempts));
                }
            }
        }
        if let Some(delay) = delay {
            state.retry_at.insert(id.to_string(), Instant::now() + delay);
        }
        save(&state);
    }
}

/// What the worker should do now.
#[derive(Debug, PartialEq)]
enum Pick {
    /// Start the job at this position.
    Start(usize),
    /// Nothing to start yet; look again after this long (or when woken).
    Wait(Duration),
}

/// Strict priority: the first job that is not finished, failed or paused is
/// the only candidate. If it is waiting out a retry delay, the whole queue
/// waits with it.
fn pick_next(jobs: &[Job], retry_at: &HashMap<String, Instant>, now: Instant) -> Pick {
    let Some(position) = jobs.iter().position(|job| matches!(job.status, Status::Queued | Status::Waiting | Status::Downloading)) else {
        return Pick::Wait(Duration::from_secs(30));
    };
    match retry_at.get(&jobs[position].id) {
        Some(at) if *at > now => Pick::Wait((*at - now).max(Duration::from_millis(200))),
        _ => Pick::Start(position),
    }
}

fn backoff(attempts: u32) -> Duration {
    BACKOFF_START.saturating_mul(2u32.saturating_pow(attempts.saturating_sub(1).min(8))).min(BACKOFF_CAP)
}

fn save(state: &State) {
    let manifest = Manifest { jobs: state.jobs.clone() };
    let Ok(text) = toml::to_string_pretty(&manifest) else { return };
    let path = manifest_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, text);
}

/// `<source>-<id>-<title>-<quality>.mp4` with anything filesystem-hostile
/// flattened to `-` (the source is included so ids from different sites
/// can never collide).
fn file_name_for(source: &str, id: &str, title: &str, quality: &str) -> String {
    let flat = |text: &str| -> String {
        text.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect::<String>().split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-")
    };
    let (source, id, quality) = (flat(source), flat(id), flat(quality));
    let slug: String = title
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let slug: String = slug.chars().take(60).collect();
    format!("{source}-{id}-{}-{quality}.mp4", if slug.is_empty() { "wallpaper" } else { &slug })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: &str, status: Status) -> Job {
        Job {
            id: id.into(),
            source: String::new(),
            title: String::new(),
            url: String::new(),
            file_name: String::new(),
            quality: String::new(),
            status,
            bytes_done: 0,
            bytes_total: None,
            attempts: 0,
            last_error: None,
            retry_in: None,
        }
    }

    #[test]
    fn the_top_job_goes_first_even_while_it_waits_to_retry() {
        let now = Instant::now();
        let jobs = vec![job("a", Status::Waiting), job("b", Status::Queued), job("c", Status::Queued)];
        let mut retry = HashMap::new();
        retry.insert("a".to_string(), now + Duration::from_secs(10));
        // `a` is flaking and waits 10 s: `b` and `c` must not jump ahead of it.
        assert!(matches!(pick_next(&jobs, &retry, now), Pick::Wait(wait) if wait <= Duration::from_secs(10)));
        // Once its delay is over, `a` is the one that starts.
        assert_eq!(pick_next(&jobs, &retry, now + Duration::from_secs(11)), Pick::Start(0));
    }

    #[test]
    fn paused_failed_and_finished_jobs_are_skipped() {
        let now = Instant::now();
        let jobs = vec![job("a", Status::Paused), job("b", Status::Failed), job("c", Status::Done), job("d", Status::Queued)];
        assert_eq!(pick_next(&jobs, &HashMap::new(), now), Pick::Start(3));
        let all_held = vec![job("a", Status::Paused)];
        assert!(matches!(pick_next(&all_held, &HashMap::new(), now), Pick::Wait(_)));
    }

    #[test]
    fn backoff_grows_then_caps() {
        assert_eq!(backoff(1), Duration::from_secs(5));
        assert_eq!(backoff(2), Duration::from_secs(10));
        assert_eq!(backoff(3), Duration::from_secs(20));
        assert_eq!(backoff(50), BACKOFF_CAP);
    }

    #[test]
    fn file_names_are_filesystem_safe() {
        assert_eq!(file_name_for("motionbgs.com", "10118", "Wandering Samurai", "4K"), "motionbgs-com-10118-wandering-samurai-4k.mp4");
        assert_eq!(file_name_for("x", "9", "../../etc/passwd", "HD"), "x-9-etc-passwd-hd.mp4");
        assert_eq!(file_name_for("x", "7", "★★★", "HD"), "x-7-wallpaper-hd.mp4");
    }
}
