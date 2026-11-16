//! Downloads an agent asked for. Each runs on its own thread and ends in
//! the library; a call waits a while for it and otherwise hands back a job
//! the agent can check on, because agents give up on slow calls.

use std::hash::{Hash, Hasher};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use paperloom_fetch::FetchError;
use serde_json::{json, Value};

use crate::tools::library::{add_file, After};

pub struct Spec {
    /// `SourceSetting::id` of the site (decides how a page is read), or "link".
    pub source: String,
    pub url: String,
    pub title: String,
    pub after: After,
}

#[derive(Clone)]
struct Job {
    id: u32,
    title: String,
    url: String,
    /// downloading | importing | done | failed
    state: &'static str,
    done: u64,
    total: Option<u64>,
    message: String,
    wallpaper: Option<Value>,
}

static JOBS: Mutex<Vec<Job>> = Mutex::new(Vec::new());

fn jobs() -> std::sync::MutexGuard<'static, Vec<Job>> {
    JOBS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn update(id: u32, change: impl FnOnce(&mut Job)) {
    if let Some(job) = jobs().iter_mut().find(|job| job.id == id) {
        change(job);
    }
}

fn describe(job: &Job) -> Value {
    json!({
        "job": job.id,
        "title": job.title,
        "url": job.url,
        "state": job.state,
        "downloaded_bytes": job.done,
        "total_bytes": job.total,
        "message": job.message,
        "wallpaper": job.wallpaper,
    })
}

pub fn list() -> Vec<Value> {
    jobs().iter().map(describe).collect()
}

/// Starts the download and returns its job number.
pub fn start(spec: Spec) -> u32 {
    let id = {
        let mut jobs = jobs();
        let id = jobs.iter().map(|job| job.id).max().unwrap_or(0) + 1;
        jobs.push(Job { id, title: spec.title.clone(), url: spec.url.clone(), state: "downloading", done: 0, total: None, message: String::new(), wallpaper: None });
        id
    };
    std::thread::spawn(move || run(id, spec));
    id
}

/// Waits up to `seconds` for the job to end; answers with where it stands.
pub fn wait(id: u32, seconds: u64) -> Value {
    let started = Instant::now();
    loop {
        let job = jobs().iter().find(|job| job.id == id).cloned();
        let Some(job) = job else { return json!({ "error": "no such job" }) };
        if matches!(job.state, "done" | "failed") || started.elapsed() >= Duration::from_secs(seconds) {
            let mut answer = describe(&job);
            if !matches!(job.state, "done" | "failed") {
                answer["message"] = json!("Still downloading. It will be added to the library when it finishes; check with list_downloads.");
            }
            return answer;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

fn run(id: u32, spec: Spec) {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    spec.url.hash(&mut hasher);
    // Named after the wallpaper (the library keeps this file name), with a
    // few characters of the link so two downloads never collide.
    let slug: String = spec.title.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect::<String>().split('-').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("-");
    let slug: String = slug.trim_end_matches("-mp4").chars().take(60).collect();
    let dest = paperloom_common::downloads_dir().join(format!("{}-{:08x}.mp4", if slug.is_empty() { "wallpaper" } else { slug.as_str() }, hasher.finish() as u32));
    let part = dest.with_extension("mp4.part");
    let cancel = AtomicBool::new(false);

    // A dropped connection continues from the bytes already fetched.
    let mut attempt = 0;
    let fetched = loop {
        attempt += 1;
        let outcome = paperloom_fetch::download_file(&spec.source, &spec.url, &part, &dest, &cancel, |done, total| {
            update(id, |job| {
                job.done = done;
                job.total = total;
            })
        });
        match outcome {
            Ok(()) => break Ok(()),
            Err(FetchError::Transient(why)) if attempt < 6 => {
                update(id, |job| job.message = format!("retrying ({why})"));
                std::thread::sleep(Duration::from_secs(3 * attempt));
            }
            Err(err) => break Err(err.to_string()),
        }
    };
    if let Err(why) = fetched {
        let _ = std::fs::remove_file(&part);
        update(id, |job| {
            job.state = "failed";
            job.message = format!("Paperloom couldn't get a video from that link ({why}). Download the video file yourself, then call import_file with its path.");
        });
        return;
    }

    update(id, |job| {
        job.state = "importing";
        job.message.clear();
    });
    let added = add_file(&dest, &spec.after);
    // The library has its own copy now, unless the user's import mode links files in place.
    if paperloom_common::load().import_mode != paperloom_common::ImportMode::ReferenceInPlace {
        let _ = std::fs::remove_file(&dest);
    }
    update(id, |job| match added {
        Ok(wallpaper) => {
            job.state = "done";
            job.message = if wallpaper["already_in_library"] == true { "It was already in the library.".to_string() } else { "In the library.".to_string() };
            job.wallpaper = Some(wallpaper);
        }
        Err(why) => {
            job.state = "failed";
            job.message = why;
        }
    });
}
