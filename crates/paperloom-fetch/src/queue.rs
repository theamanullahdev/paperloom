//! Persistent download queue with one background worker.
//!
//! The promise: once a wallpaper is queued, the queue always keeps its
//! reference (id, title, URL). Transient failures — network down, timeouts,
//! 429/5xx — retry forever with capped exponential backoff, resuming from
//! the `.part` file; the queue is saved to disk on every state change, so
//! quitting the app or rebooting resumes where it left off. Only a
//! permanent failure (404, not an MP4) stops retrying automatically, and
//! even then the job stays listed with a manual "Retry".

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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub source: String,
    pub title: String,
    pub url: String,
    pub file_name: String,
    pub status: Status,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
    pub attempts: u32,
    pub last_error: Option<String>,
}

impl Job {
    pub fn final_path(&self) -> PathBuf {
        downloads_dir().join(&self.file_name)
    }

    fn part_path(&self) -> PathBuf {
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
        }

        let queue = Queue { shared: Arc::new(Shared { state: Mutex::new(State { jobs, ..State::default() }), wake: Condvar::new() }) };
        let worker = queue.clone();
        std::thread::Builder::new().name("paperloom-downloads".into()).spawn(move || worker.run()).expect("spawn download worker");
        queue
    }

    /// Queues `item`. Returns `false` if that wallpaper is already queued
    /// or downloaded-but-not-yet-imported.
    pub fn add(&self, item: &OnlineItem) -> bool {
        let id = format!("{}:{}", item.source, item.id);
        let mut state = self.lock();
        if state.jobs.iter().any(|job| job.id == id) {
            return false;
        }
        state.jobs.push(Job {
            file_name: file_name_for(&item.id, &item.title),
            id,
            source: item.source.to_string(),
            title: item.title.clone(),
            url: item.download_url.clone(),
            status: Status::Queued,
            bytes_done: 0,
            bytes_total: None,
            attempts: 0,
            last_error: None,
        });
        save(&state);
        drop(state);
        self.shared.wake.notify_all();
        true
    }

    pub fn snapshot(&self) -> Vec<Job> {
        self.lock().jobs.clone()
    }

    pub fn is_busy(&self) -> bool {
        self.lock().jobs.iter().any(|job| job.status != Status::Failed && job.status != Status::Done)
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
                download(&job.url, &job.part_path(), &job.final_path(), &cancel, move |done, total| {
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

    /// Blocks until a job is eligible, marks it `Downloading`, returns it.
    fn next_job(&self) -> (Job, Arc<AtomicBool>) {
        let mut state = self.lock();
        loop {
            let now = Instant::now();
            let ready = state.jobs.iter().position(|job| {
                matches!(job.status, Status::Queued | Status::Waiting)
                    && state.retry_at.get(&job.id).is_none_or(|at| *at <= now)
            });

            if let Some(position) = ready {
                let cancel = Arc::new(AtomicBool::new(false));
                state.jobs[position].status = Status::Downloading;
                let job = state.jobs[position].clone();
                state.active = Some((job.id.clone(), cancel.clone()));
                save(&state);
                return (job, cancel);
            }

            let next_retry = state.retry_at.values().min().map(|at| at.saturating_duration_since(now));
            let wait = next_retry.unwrap_or(Duration::from_secs(30)).max(Duration::from_millis(200));
            state = self.shared.wake.wait_timeout(state, wait).unwrap_or_else(|poisoned| poisoned.into_inner()).0;
        }
    }

    fn finish(&self, id: &str, result: Result<(), FetchError>) {
        let mut state = self.lock();
        state.active = None;
        let mut delay = None;
        if let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) {
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

/// `<id>-<title>.mp4` with anything filesystem-hostile flattened to `-`.
fn file_name_for(id: &str, title: &str) -> String {
    let slug: String = title
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let slug: String = slug.chars().take(60).collect();
    format!("{id}-{}.mp4", if slug.is_empty() { "wallpaper" } else { &slug })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_then_caps() {
        assert_eq!(backoff(1), Duration::from_secs(5));
        assert_eq!(backoff(2), Duration::from_secs(10));
        assert_eq!(backoff(3), Duration::from_secs(20));
        assert_eq!(backoff(50), BACKOFF_CAP);
    }

    #[test]
    fn file_names_are_filesystem_safe() {
        assert_eq!(file_name_for("10118", "Wandering Samurai"), "10118-wandering-samurai.mp4");
        assert_eq!(file_name_for("9", "../../etc/passwd"), "9-etc-passwd.mp4");
        assert_eq!(file_name_for("7", "★★★"), "7-wallpaper.mp4");
    }
}
