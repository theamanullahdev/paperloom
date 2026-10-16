//! One shared HTTP agent plus process-wide politeness throttles (see
//! `Lane`): every request goes through `wait_turn`, so the site never sees
//! requests faster than the lane's gap no matter how many threads ask.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const USER_AGENT: &str = concat!("Paperloom/", env!("CARGO_PKG_VERSION"), " (+https://github.com/theamanullahdev/paperloom)");

pub fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .user_agent(USER_AGENT)
            // Status codes are handled by callers (429/5xx mean "retry",
            // 404 means "gone"), so don't turn them into transport errors.
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(15)))
            // Per-read stall limit, not a whole-download limit — a big
            // file on a slow link is fine, a connection that has gone
            // silent for 30s is not.
            .timeout_recv_body(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .build()
            .into()
    })
}

/// Separate politeness lanes: page loads are the expensive ones for the
/// site, preview images come from its CDN cache, and big file downloads
/// start rarely. A process-wide gap per lane holds no matter how many
/// threads the GUI spawns.
#[derive(Clone, Copy)]
pub enum Lane {
    Page,
    Image,
    Download,
}

impl Lane {
    fn gap(self) -> Duration {
        match self {
            Lane::Page => Duration::from_millis(700),
            Lane::Image => Duration::from_millis(120),
            Lane::Download => Duration::from_millis(1500),
        }
    }

    fn clock(self) -> &'static Mutex<Option<Instant>> {
        static PAGE: Mutex<Option<Instant>> = Mutex::new(None);
        static IMAGE: Mutex<Option<Instant>> = Mutex::new(None);
        static DOWNLOAD: Mutex<Option<Instant>> = Mutex::new(None);
        match self {
            Lane::Page => &PAGE,
            Lane::Image => &IMAGE,
            Lane::Download => &DOWNLOAD,
        }
    }
}

/// Blocks until at least the lane's gap has passed since the previous
/// request on that lane.
pub fn wait_turn(lane: Lane) {
    let mut last = lane.clock().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(previous) = *last {
        let elapsed = previous.elapsed();
        if elapsed < lane.gap() {
            std::thread::sleep(lane.gap() - elapsed);
        }
    }
    *last = Some(Instant::now());
}

#[derive(Debug)]
pub enum FetchError {
    /// Worth retrying later (network down, 429, 5xx, timeouts).
    Transient(String),
    /// Retrying won't help (404/410/other 4xx).
    Permanent(String),
    /// No internet connection at all (checked up front, so callers can say
    /// so immediately instead of waiting for a timeout).
    Offline,
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Transient(msg) | FetchError::Permanent(msg) => f.write_str(msg),
            FetchError::Offline => f.write_str("No internet connection"),
        }
    }
}

impl std::error::Error for FetchError {}

pub fn classify_status(status: u16) -> Option<FetchError> {
    match status {
        200..=299 => None,
        408 | 425 | 429 | 500..=599 => Some(FetchError::Transient(format!("server returned HTTP {status}"))),
        _ => Some(FetchError::Permanent(format!("server returned HTTP {status}"))),
    }
}

pub fn classify_transport(err: ureq::Error) -> FetchError {
    FetchError::Transient(err.to_string())
}

/// Fetches a page as text, returning the body and the final URL after
/// redirects (needed to page through search results).
pub fn get_text(url: &str) -> Result<(String, String), FetchError> {
    get_text_with(url, &[])
}

/// Like `get_text`, with extra request headers (API keys).
pub fn get_text_with(url: &str, headers: &[(&str, &str)]) -> Result<(String, String), FetchError> {
    use ureq::ResponseExt;

    wait_turn(Lane::Page);
    let mut request = agent().get(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let mut response = request.call().map_err(classify_transport)?;
    let status = response.status().as_u16();
    if status == 401 || status == 403 {
        return Err(FetchError::Permanent("the API key was rejected (check it in Sources)".into()));
    }
    if let Some(err) = classify_status(status) {
        return Err(err);
    }
    let final_url = response.get_uri().to_string();
    let body = response.body_mut().read_to_string().map_err(classify_transport)?;
    Ok((body, final_url))
}

/// Fetches small binary content (preview images) fully into memory.
pub fn get_bytes(url: &str, limit: u64) -> Result<Vec<u8>, FetchError> {
    wait_turn(Lane::Image);
    let mut response = agent().get(url).call().map_err(classify_transport)?;
    if let Some(err) = classify_status(response.status().as_u16()) {
        return Err(err);
    }
    response.body_mut().with_config().limit(limit).read_to_vec().map_err(classify_transport)
}
