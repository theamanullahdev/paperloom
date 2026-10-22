//! App self-update: asks GitHub Releases what's newer than this build,
//! picks the installer that matches how this copy was installed, downloads
//! it (resumable, SHA-256 verified) and hands it to the OS installer.
//!
//! Releases are produced by `.github/workflows/release.yml`, which names
//! every asset `paperloom-<version>-<os>-<arch>[-setup].<ext>` — that
//! naming is the contract `Release::asset_for` matches on — and publishes a
//! `SHA256SUMS.txt` alongside them (GitHub's own per-asset `digest` is
//! preferred when present).
//!
//! Nothing here installs silently: the downloaded file is opened with the
//! platform's normal installer (Software Manager / GDebi for a .deb, the
//! setup wizard or `msiexec` on Windows, Finder for a .dmg), so the user
//! still confirms the install the usual way.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::http::{self, classify_status, classify_transport, FetchError};

pub const REPOSITORY: &str = "theamanullahdev/paperloom";

/// The release list mirrored onto GitHub Pages by `apt-repo.yml` after
/// every release — the same JSON as the API, but a static file, so it isn't
/// subject to the API's 60-requests-an-hour-per-network limit (which a
/// busy network, or a few clicks of "Check now", can use up).
pub fn releases_mirror_url() -> String {
    let (owner, repo) = REPOSITORY.split_once('/').expect("owner/repo");
    format!("https://{owner}.github.io/{repo}/releases.json")
}

pub fn releases_api_url() -> String {
    format!("https://api.github.com/repos/{REPOSITORY}/releases?per_page=30")
}

/// Where the README's apt setup writes the Paperloom repository entry.
pub const APT_SOURCE_FILE: &str = "/etc/apt/sources.list.d/paperloom.list";

pub fn releases_page_url() -> String {
    format!("https://github.com/{REPOSITORY}/releases")
}

// --- Versions -------------------------------------------------------------

/// `major.minor.patch[-pre]`, compared the semver way (a pre-release sorts
/// before its release). Enough for our own tags; not a general semver crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Option<String>,
}

impl Version {
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().trim_start_matches('v');
        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) => (core, Some(pre.to_string())),
            None => (text, None),
        };
        let core = core.split('+').next()?;
        let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
        let version = Self { major: parts.next()??, minor: parts.next()??, patch: parts.next()??, pre };
        parts.next().is_none().then_some(version)
    }

    /// The version of this build.
    pub fn current() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).expect("the crate version is valid semver")
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch)).then_with(|| match (&self.pre, &other.pre) {
            (None, None) => std::cmp::Ordering::Equal,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(_), None) => std::cmp::Ordering::Less,
            (Some(a), Some(b)) => a.cmp(b),
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(pre) = &self.pre {
            write!(f, "-{pre}")?;
        }
        Ok(())
    }
}

// --- Releases -------------------------------------------------------------

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    published_at: Option<String>,
    html_url: String,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    size: u64,
    browser_download_url: String,
    content_type: Option<String>,
    #[serde(default)]
    download_count: u64,
    /// "sha256:<hex>" — GitHub computes this for every uploaded asset.
    digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub name: String,
    pub size: u64,
    pub url: String,
    pub content_type: String,
    pub download_count: u64,
    /// Lower-case hex, from GitHub's asset digest when it provides one.
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub version: Version,
    pub tag: String,
    pub title: String,
    pub notes: String,
    pub prerelease: bool,
    pub published_at: String,
    pub page_url: String,
    pub assets: Vec<Asset>,
}

pub const CHECKSUMS_ASSET: &str = "SHA256SUMS.txt";

impl Release {
    /// The installer for `kind` on this CPU architecture, if this release has one.
    pub fn asset_for(&self, kind: InstallKind) -> Option<&Asset> {
        let suffix = kind.asset_suffix()?;
        self.platform_assets().into_iter().find(|asset| asset.name.ends_with(suffix))
    }

    /// Every asset built for this OS and architecture (all installer formats).
    pub fn platform_assets(&self) -> Vec<&Asset> {
        let os = os_token();
        let arch = std::env::consts::ARCH;
        self.assets.iter().filter(|asset| asset.name.contains(&format!("-{os}-{arch}"))).collect()
    }

    pub fn checksums_url(&self) -> Option<String> {
        self.assets.iter().find(|asset| asset.name == CHECKSUMS_ASSET).map(|asset| asset.url.clone())
    }
}

fn os_token() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        _ => "linux",
    }
}

/// Everything one check found out, kept whole so the Updates page can show
/// exactly what happened (this page is for people who want the details).
#[derive(Debug, Clone)]
pub struct CheckReport {
    pub current: Version,
    /// Published releases (drafts never; pre-releases only if asked for),
    /// newest first.
    pub releases: Vec<Release>,
    /// Where the release list came from (the Pages mirror, or the API).
    pub api_url: String,
    pub http_status: u16,
    /// Why the mirror wasn't used, when the API answered instead.
    pub mirror_error: Option<String>,
    /// GitHub's unauthenticated API allows 60 requests an hour per IP
    /// (unknown when the mirror answered).
    pub rate_limit_remaining: Option<String>,
    pub rate_limit_reset: Option<u64>,
    pub took: Duration,
}

impl CheckReport {
    /// The newest release that's newer than this build and not the version
    /// the user chose to skip.
    pub fn available(&self, ignored: Option<&str>) -> Option<&Release> {
        let ignored = ignored.and_then(Version::parse);
        self.releases.iter().find(|release| release.version > self.current && Some(&release.version) != ignored.as_ref())
    }

    /// Newer than this build at all, ignored or not.
    pub fn newest_newer(&self) -> Option<&Release> {
        self.releases.iter().find(|release| release.version > self.current)
    }
}

/// Fetches the release list: the Pages mirror first, the API if the mirror
/// is missing or broken.
pub fn check(include_prereleases: bool) -> Result<CheckReport, FetchError> {
    if !crate::net::is_online() {
        return Err(FetchError::Offline);
    }
    let started = Instant::now();
    let mirror_error = match fetch_mirror(include_prereleases) {
        // A mirror that lists no releases at all is broken (there is always at
        // least the one you are running): ask GitHub directly instead.
        Ok((_, releases)) if releases.is_empty() => "the release list was empty".to_string(),
        Ok((url, releases)) => {
            return Ok(CheckReport {
                current: Version::current(),
                releases,
                api_url: url,
                http_status: 200,
                mirror_error: None,
                rate_limit_remaining: None,
                rate_limit_reset: None,
                took: started.elapsed(),
            });
        }
        Err(err) => err.to_string(),
    };
    let mut report = check_api(include_prereleases)?;
    report.mirror_error = Some(mirror_error);
    report.took = started.elapsed();
    Ok(report)
}

fn fetch_mirror(include_prereleases: bool) -> Result<(String, Vec<Release>), FetchError> {
    // A changing query string gets past any proxy cache in between.
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let url = releases_mirror_url();
    let mut response = http::agent().get(&format!("{url}?t={secs}")).call().map_err(classify_transport)?;
    if let Some(err) = classify_status(response.status().as_u16()) {
        return Err(err);
    }
    let body = response.body_mut().read_to_string().map_err(classify_transport)?;
    let releases = parse_releases(&body, include_prereleases).map_err(|err| FetchError::Permanent(format!("unreadable release list: {err}")))?;
    Ok((url, releases))
}

fn check_api(include_prereleases: bool) -> Result<CheckReport, FetchError> {
    let started = Instant::now();
    let url = releases_api_url();
    let mut response = http::agent()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .call()
        .map_err(classify_transport)?;
    let status = response.status().as_u16();
    let header = |name: &str| response.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
    let rate_limit_remaining = header("x-ratelimit-remaining");
    let rate_limit_reset = header("x-ratelimit-reset").and_then(|v| v.parse().ok());
    if status == 403 && rate_limit_remaining.as_deref() == Some("0") {
        return Err(FetchError::Transient("GitHub's hourly API limit for this network is used up; try again later".into()));
    }
    if status == 404 {
        return Err(FetchError::Permanent(format!("{REPOSITORY} wasn't found on GitHub (is the repository public?)")));
    }
    if let Some(err) = classify_status(status) {
        return Err(err);
    }
    let body = response.body_mut().read_to_string().map_err(classify_transport)?;
    let releases = parse_releases(&body, include_prereleases).map_err(|err| FetchError::Permanent(format!("unexpected reply from GitHub: {err}")))?;

    Ok(CheckReport {
        current: Version::current(),
        releases,
        api_url: url,
        http_status: status,
        mirror_error: None,
        rate_limit_remaining,
        rate_limit_reset,
        took: started.elapsed(),
    })
}

fn parse_releases(json: &str, include_prereleases: bool) -> Result<Vec<Release>, serde_json::Error> {
    let api: Vec<ApiRelease> = serde_json::from_str(json)?;
    let mut releases: Vec<Release> = api
        .into_iter()
        .filter(|release| !release.draft && (include_prereleases || !release.prerelease))
        .filter_map(|release| {
            let version = Version::parse(&release.tag_name)?;
            Some(Release {
                title: release.name.filter(|name| !name.trim().is_empty()).unwrap_or_else(|| release.tag_name.clone()),
                tag: release.tag_name,
                version,
                notes: release.body.unwrap_or_default(),
                prerelease: release.prerelease,
                published_at: release.published_at.unwrap_or_default(),
                page_url: release.html_url,
                assets: release
                    .assets
                    .into_iter()
                    .map(|asset| Asset {
                        sha256: asset.digest.and_then(|d| d.strip_prefix("sha256:").map(str::to_ascii_lowercase)),
                        name: asset.name,
                        size: asset.size,
                        url: asset.browser_download_url,
                        content_type: asset.content_type.unwrap_or_default(),
                        download_count: asset.download_count,
                    })
                    .collect(),
            })
        })
        .collect();
    releases.sort_by(|a, b| b.version.cmp(&a.version));
    Ok(releases)
}

// --- How this copy was installed -----------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallKind {
    Deb,
    Rpm,
    /// Updated by Flatpak itself (Software Manager / `flatpak update`).
    Flatpak,
    WindowsSetup,
    WindowsMsi,
    MacDmg,
}

impl InstallKind {
    pub const ALL: [InstallKind; 6] =
        [InstallKind::Deb, InstallKind::Rpm, InstallKind::Flatpak, InstallKind::WindowsSetup, InstallKind::WindowsMsi, InstallKind::MacDmg];

    /// Best guess from where and how this binary lives on disk.
    pub fn detect() -> Self {
        Self::detect_with_reason().0
    }

    /// The guess plus why — shown on the Updates page.
    pub fn detect_with_reason() -> (Self, String) {
        let exe = std::env::current_exe().unwrap_or_default();
        match std::env::consts::OS {
            "windows" => {
                let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
                if dir.join("unins000.exe").is_file() {
                    (InstallKind::WindowsSetup, format!("setup uninstaller found in {}", dir.display()))
                } else {
                    (InstallKind::WindowsMsi, format!("no setup uninstaller in {} — assuming the MSI", dir.display()))
                }
            }
            "macos" => (InstallKind::MacDmg, "macOS app".into()),
            _ => {
                if let Ok(id) = std::env::var("FLATPAK_ID") {
                    return (InstallKind::Flatpak, format!("running inside Flatpak ({id})"));
                }
                if Path::new("/var/lib/dpkg/info/paperloom.list").is_file() {
                    let reason = if Path::new(APT_SOURCE_FILE).is_file() {
                        "dpkg has the paperloom package, and the Paperloom apt repository is set up — your normal system updates (apt upgrade / Update Manager) install new versions"
                    } else {
                        "dpkg has the paperloom package"
                    };
                    return (InstallKind::Deb, reason.into());
                }
                let rpm_owned = std::process::Command::new("rpm").args(["-q", "paperloom"]).output().is_ok_and(|out| out.status.success());
                if rpm_owned {
                    return (InstallKind::Rpm, "rpm has the paperloom package".into());
                }
                let os_release = fs::read_to_string("/etc/os-release").unwrap_or_default();
                let rpm_family = os_release
                    .lines()
                    .filter(|line| line.starts_with("ID=") || line.starts_with("ID_LIKE="))
                    .any(|line| ["fedora", "rhel", "suse", "centos"].iter().any(|distro| line.contains(distro)));
                if rpm_family {
                    (InstallKind::Rpm, format!("not installed as a package ({}); this distro uses .rpm", exe.display()))
                } else {
                    (InstallKind::Deb, format!("not installed as a package ({}); this distro uses .deb", exe.display()))
                }
            }
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            InstallKind::Deb => ".deb package",
            InstallKind::Rpm => ".rpm package",
            InstallKind::Flatpak => "Flatpak",
            InstallKind::WindowsSetup => "Windows setup (.exe)",
            InstallKind::WindowsMsi => "Windows installer (.msi)",
            InstallKind::MacDmg => "macOS disk image (.dmg)",
        }
    }

    fn asset_suffix(self) -> Option<&'static str> {
        match self {
            InstallKind::Deb => Some(".deb"),
            InstallKind::Rpm => Some(".rpm"),
            InstallKind::Flatpak => None,
            InstallKind::WindowsSetup => Some("-setup.exe"),
            InstallKind::WindowsMsi => Some(".msi"),
            InstallKind::MacDmg => Some(".dmg"),
        }
    }

    /// Whether the app downloads and installs updates itself (Flatpak
    /// installs are updated by Flatpak, and the sandbox couldn't anyway).
    pub fn self_updates(self) -> bool {
        self != InstallKind::Flatpak
    }

    /// The kind an asset file is, from its name.
    pub fn of_asset(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.asset_suffix().is_some_and(|suffix| name.ends_with(suffix)))
    }
}

// --- Downloading ----------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Downloading,
    /// Waiting before the next attempt after a network error.
    Retrying,
    Verifying,
    Ready,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct DownloadState {
    pub asset: Asset,
    pub version: Version,
    pub path: PathBuf,
    pub part_path: PathBuf,
    pub phase: Phase,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
    pub attempts: u32,
    /// Where the expected checksum came from ("GitHub asset digest" /
    /// "SHA256SUMS.txt"), or `None` when no checksum was published.
    pub checksum_source: Option<&'static str>,
    pub expected_sha256: Option<String>,
    pub actual_sha256: Option<String>,
    pub message: Option<String>,
    pub started: Instant,
    pub finished: Option<Instant>,
}

/// A download running on its own thread; the GUI polls `snapshot`.
pub struct UpdateDownload {
    state: Arc<Mutex<DownloadState>>,
    cancel: Arc<AtomicBool>,
}

const MAX_ATTEMPTS: u32 = 6;

impl UpdateDownload {
    pub fn start(release: &Release, asset: &Asset) -> Self {
        let dir = paperloom_common::updates_dir();
        let path = dir.join(&asset.name);
        let state = DownloadState {
            asset: asset.clone(),
            version: release.version.clone(),
            part_path: dir.join(format!("{}.part", asset.name)),
            path,
            phase: Phase::Downloading,
            bytes_done: 0,
            bytes_total: Some(asset.size),
            attempts: 0,
            checksum_source: asset.sha256.as_ref().map(|_| "GitHub asset digest"),
            expected_sha256: asset.sha256.clone(),
            actual_sha256: None,
            message: None,
            started: Instant::now(),
            finished: None,
        };
        let state = Arc::new(Mutex::new(state));
        let cancel = Arc::new(AtomicBool::new(false));
        let checksums_url = release.checksums_url();
        {
            let (state, cancel) = (state.clone(), cancel.clone());
            std::thread::spawn(move || run_download(&state, &cancel, checksums_url));
        }
        Self { state, cancel }
    }

    pub fn snapshot(&self) -> DownloadState {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_active(&self) -> bool {
        matches!(self.snapshot().phase, Phase::Downloading | Phase::Retrying | Phase::Verifying)
    }
}

fn run_download(state: &Mutex<DownloadState>, cancel: &AtomicBool, checksums_url: Option<String>) {
    let update = |f: &dyn Fn(&mut DownloadState)| f(&mut state.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
    let (url, part, path, name) = {
        let s = state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        (s.asset.url.clone(), s.part_path.clone(), s.path.clone(), s.asset.name.clone())
    };

    // No digest from the API: fall back to the release's SHA256SUMS.txt.
    let needs_sums = state.lock().map(|s| s.expected_sha256.is_none()).unwrap_or(true);
    if needs_sums {
        if let Some(sums_url) = &checksums_url {
            if let Ok((body, _)) = http::get_text(sums_url) {
                if let Some(hash) = parse_checksums(&body, &name) {
                    update(&|s| {
                        s.expected_sha256 = Some(hash.clone());
                        s.checksum_source = Some(CHECKSUMS_ASSET);
                    });
                }
            }
        }
    }

    let mut delay = Duration::from_secs(2);
    loop {
        update(&|s| {
            s.attempts += 1;
            s.phase = Phase::Downloading;
        });
        let result = attempt(&url, &part, cancel, |done, total| {
            update(&|s| {
                s.bytes_done = done;
                if total.is_some() {
                    s.bytes_total = total;
                }
            })
        });
        match result {
            Ok(()) => break,
            Err(_) if cancel.load(Ordering::Relaxed) => {
                let _ = fs::remove_file(&part);
                update(&|s| {
                    s.phase = Phase::Cancelled;
                    s.message = Some("Cancelled".into());
                    s.finished = Some(Instant::now());
                });
                return;
            }
            Err(FetchError::Permanent(message)) => return fail(state, message),
            Err(err) => {
                let attempts = state.lock().map(|s| s.attempts).unwrap_or(MAX_ATTEMPTS);
                if attempts >= MAX_ATTEMPTS {
                    return fail(state, format!("{err} (gave up after {attempts} attempts)"));
                }
                let message = format!("{err} — retrying in {}s", delay.as_secs());
                update(&|s| {
                    s.phase = Phase::Retrying;
                    s.message = Some(message.clone());
                });
                std::thread::sleep(delay);
                delay = (delay * 2).min(Duration::from_secs(60));
            }
        }
    }

    update(&|s| s.phase = Phase::Verifying);
    let actual = match sha256_file(&part) {
        Ok(hash) => hash,
        Err(err) => return fail(state, format!("couldn't read the download: {err}")),
    };
    let expected = state.lock().ok().and_then(|s| s.expected_sha256.clone());
    if let Some(expected) = &expected {
        if *expected != actual {
            let _ = fs::remove_file(&part);
            update(&|s| s.actual_sha256 = Some(actual.clone()));
            return fail(state, "checksum mismatch — the file was deleted; try again".into());
        }
    }
    if let Err(err) = fs::rename(&part, &path) {
        return fail(state, format!("couldn't save the download: {err}"));
    }
    update(&|s| {
        s.actual_sha256 = Some(actual.clone());
        s.phase = Phase::Ready;
        s.message = Some(if expected.is_some() { "Verified".into() } else { "Downloaded (no published checksum to verify against)".into() });
        s.finished = Some(Instant::now());
    });
    prune_downloads(&path);
}

/// Keeps the updates folder small: the installer just downloaded (`keep`)
/// and the newest one older than it (to go back to), nothing else.
/// Leftover partial downloads go too. Files that aren't Paperloom
/// installers are left alone.
pub fn prune_downloads(keep: &Path) {
    let Some(dir) = keep.parent() else { return };
    let Ok(entries) = fs::read_dir(dir) else { return };
    let version_of = |name: &str| name.strip_prefix("paperloom-")?.split('-').next().and_then(Version::parse);
    let Some(kept) = keep.file_name().and_then(|name| version_of(&name.to_string_lossy())) else { return };

    let files: Vec<(PathBuf, String)> =
        entries.flatten().map(|entry| (entry.path(), entry.file_name().to_string_lossy().into_owned())).filter(|(path, _)| path.is_file()).collect();
    let previous = files.iter().filter(|(_, name)| !name.ends_with(".part")).filter_map(|(_, name)| version_of(name)).filter(|version| *version < kept).max();
    for (path, name) in &files {
        if path == keep || !name.starts_with("paperloom-") {
            continue;
        }
        let stale = name.ends_with(".part") || version_of(name).is_some_and(|version| Some(&version) != previous.as_ref());
        if stale {
            let _ = fs::remove_file(path);
        }
    }
}

fn fail(state: &Mutex<DownloadState>, message: String) {
    let mut s = state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    s.phase = Phase::Failed;
    s.message = Some(message);
    s.finished = Some(Instant::now());
}

/// One resumable attempt into `part` (continues it with `Range` if it exists).
fn attempt(url: &str, part: &Path, cancel: &AtomicBool, mut progress: impl FnMut(u64, Option<u64>)) -> Result<(), FetchError> {
    let io = |err: std::io::Error| FetchError::Transient(format!("disk error: {err}"));
    if let Some(parent) = part.parent() {
        fs::create_dir_all(parent).map_err(io)?;
    }
    let existing = part.metadata().map(|m| m.len()).unwrap_or(0);

    let mut request = http::agent().get(url);
    if existing > 0 {
        request = request.header("Range", format!("bytes={existing}-"));
    }
    let mut response = request.call().map_err(classify_transport)?;
    let status = response.status().as_u16();
    let content_length = response.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    let (mut done, total, append) = match status {
        206 => (existing, content_length.map(|len| len + existing), true),
        200 => (0, content_length, false),
        416 => {
            let _ = fs::remove_file(part);
            return Err(FetchError::Transient("resume offset rejected, restarting".into()));
        }
        other => return Err(classify_status(other).unwrap_or_else(|| FetchError::Transient(format!("unexpected HTTP {other}")))),
    };

    let mut file = OpenOptions::new().create(true).write(true).append(append).truncate(!append).open(part).map_err(io)?;
    let mut reader = response.body_mut().as_reader();
    let mut buffer = vec![0u8; 64 * 1024];
    progress(done, total);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(FetchError::Transient("cancelled".into()));
        }
        let read = reader.read(&mut buffer).map_err(|err| FetchError::Transient(err.to_string()))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read]).map_err(io)?;
        done += read as u64;
        progress(done, total);
    }
    file.flush().map_err(io)?;
    match total {
        Some(total) if done != total => Err(FetchError::Transient(format!("connection ended at {done} of {total} bytes"))),
        _ => Ok(()),
    }
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

/// `sha256sum`-format lines: "<hex>  <name>" (a `*` before the name marks
/// binary mode).
fn parse_checksums(body: &str, name: &str) -> Option<String> {
    body.lines().find_map(|line| {
        let (hash, file) = line.trim().split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name && hash.len() == 64).then(|| hash.to_ascii_lowercase())
    })
}

// --- Installing -----------------------------------------------------------

/// What the app should do after the installer has been started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterLaunch {
    /// Windows: the installer must replace our running executables, so the
    /// app (and engine) should exit now.
    ExitApp,
    /// The installer opened in its own window; the new version applies once
    /// the user finishes there and restarts Paperloom.
    KeepRunning,
    /// Linux: the package is installed (after the system password prompt);
    /// restarting Paperloom switches to it.
    Installed,
}

/// Installs the downloaded update. On Linux that's the system's own
/// password prompt (polkit, via `pkexec`) and then the package manager,
/// without opening a separate installer app; elsewhere (or if that isn't
/// possible) the platform's installer UI. Blocks until a Linux install
/// finishes, so run it off the UI thread.
pub fn launch_installer(path: &Path) -> Result<AfterLaunch, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let spawn = |program: &str, args: &[&std::ffi::OsStr]| {
        std::process::Command::new(program).args(args).spawn().map(|_| ()).map_err(|err| format!("couldn't start {program}: {err}"))
    };
    match InstallKind::of_asset(&name) {
        Some(InstallKind::WindowsSetup) => {
            std::process::Command::new(path).spawn().map_err(|err| format!("couldn't start the installer: {err}"))?;
            Ok(AfterLaunch::ExitApp)
        }
        Some(InstallKind::WindowsMsi) => {
            spawn("msiexec", &["/i".as_ref(), path.as_os_str()])?;
            Ok(AfterLaunch::ExitApp)
        }
        Some(InstallKind::MacDmg) => {
            spawn("open", &[path.as_os_str()])?;
            Ok(AfterLaunch::KeepRunning)
        }
        kind => match install_with_password_prompt(kind, path) {
            Some(result) => result.map(|()| AfterLaunch::Installed),
            None => {
                // No pkexec / package manager / polkit agent: the desktop's
                // package installer (GDebi, Software Manager…) instead.
                spawn("xdg-open", &[path.as_os_str()])?;
                Ok(AfterLaunch::KeepRunning)
            }
        },
    }
}

/// `pkexec <package manager> install <file>`. `None` when this can't work
/// here (nothing to ask for the password, no known package manager), so the
/// caller falls back to the desktop's installer app.
fn install_with_password_prompt(kind: Option<InstallKind>, path: &Path) -> Option<Result<(), String>> {
    let on_path = |program: &str| std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()));
    if !on_path("pkexec") {
        return None;
    }
    let file = path.as_os_str();
    let command: Vec<&std::ffi::OsStr> = match kind {
        Some(InstallKind::Deb) if on_path("apt-get") => // apt normally reads local files as its unprivileged `_apt` user,
        // which can't open a file in the user's home folder — and when the
        // same version is also in the apt repository it tries that "file
        // not found" copy and gives up. Reading as root avoids both.
        vec!["apt-get".as_ref(), "install".as_ref(), "-y".as_ref(), "--allow-downgrades".as_ref(), "-o".as_ref(), "APT::Sandbox::User=root".as_ref(), file],
        Some(InstallKind::Deb) if on_path("dpkg") => vec!["dpkg".as_ref(), "-i".as_ref(), file],
        Some(InstallKind::Rpm) if on_path("dnf") => vec!["dnf".as_ref(), "install".as_ref(), "-y".as_ref(), file],
        Some(InstallKind::Rpm) if on_path("zypper") => {
            vec!["zypper".as_ref(), "--non-interactive".as_ref(), "install".as_ref(), "--allow-unsigned-rpm".as_ref(), file]
        }
        Some(InstallKind::Rpm) if on_path("rpm") => vec!["rpm".as_ref(), "-U".as_ref(), "--force".as_ref(), file],
        _ => return None,
    };
    let output = match std::process::Command::new("pkexec").args(&command).stdin(std::process::Stdio::null()).output() {
        Ok(output) => output,
        Err(_) => return None,
    };
    if output.status.success() {
        return Some(Ok(()));
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("authentication agent") {
        return None; // No password dialog available on this desktop.
    }
    // pkexec: 126 = the password prompt was dismissed, 127 = not authorized.
    Some(Err(match output.status.code() {
        Some(126) => "the password prompt was cancelled".into(),
        Some(127) => "not authorized to install software".into(),
        _ => {
            let tail: Vec<&str> = stderr.lines().filter(|line| !line.trim().is_empty()).collect();
            let tail = tail[tail.len().saturating_sub(3)..].join(" / ");
            format!("{} failed: {}", command[0].to_string_lossy(), if tail.is_empty() { output.status.to_string() } else { tail })
        }
    }))
}

/// Opens a URL (release page) in the default browser.
pub fn open_url(url: &str) -> std::io::Result<()> {
    let program = match std::env::consts::OS {
        "windows" => {
            let mut command = std::process::Command::new("cmd");
            paperloom_common::process::hide_console(&mut command);
            command.args(["/C", "start", "", url]).spawn()?;
            return Ok(());
        }
        "macos" => "open",
        _ => "xdg-open",
    };
    std::process::Command::new(program).arg(url).spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_like_semver() {
        let v = |s| Version::parse(s).unwrap();
        assert!(v("0.2.0") > v("0.1.9"));
        assert!(v("v1.0.0") > v("0.99.99"));
        assert!(v("1.0.0") > v("1.0.0-rc.1"));
        assert!(v("1.0.0-rc.2") > v("1.0.0-rc.1"));
        assert_eq!(v("v0.3.1").to_string(), "0.3.1");
        assert_eq!(Version::parse("1.2"), None);
        assert_eq!(Version::parse("nightly"), None);
    }

    const SAMPLE: &str = r#"[
      {"tag_name":"v0.3.0","name":"v0.3.0","body":"Notes","draft":false,"prerelease":false,
       "published_at":"2026-10-01T00:00:00Z","html_url":"https://github.com/x/y/releases/tag/v0.3.0",
       "assets":[
         {"name":"paperloom-0.3.0-linux-x86_64.deb","size":10,"browser_download_url":"https://e/deb","content_type":"application/vnd.debian.binary-package","download_count":3,"digest":"sha256:ABCDEF0000000000000000000000000000000000000000000000000000000000"},
         {"name":"paperloom-0.3.0-windows-x86_64-setup.exe","size":20,"browser_download_url":"https://e/exe","download_count":1},
         {"name":"paperloom-0.3.0-windows-x86_64.msi","size":20,"browser_download_url":"https://e/msi"},
         {"name":"paperloom-0.3.0-macos-aarch64.dmg","size":30,"browser_download_url":"https://e/dmg"},
         {"name":"SHA256SUMS.txt","size":1,"browser_download_url":"https://e/sums"}
       ]},
      {"tag_name":"v0.4.0-beta.1","draft":false,"prerelease":true,"html_url":"h","assets":[]},
      {"tag_name":"v9.9.9","draft":true,"prerelease":false,"html_url":"h","assets":[]},
      {"tag_name":"v0.0.1","draft":false,"prerelease":false,"html_url":"h","assets":[]}
    ]"#;

    #[test]
    fn parses_and_filters_releases() {
        let stable = parse_releases(SAMPLE, false).unwrap();
        assert_eq!(stable.iter().map(|r| r.tag.as_str()).collect::<Vec<_>>(), ["v0.3.0", "v0.0.1"]);
        let all = parse_releases(SAMPLE, true).unwrap();
        assert_eq!(all[0].tag, "v0.4.0-beta.1");
        let release = &stable[0];
        assert_eq!(release.assets[0].sha256.as_deref(), Some("abcdef0000000000000000000000000000000000000000000000000000000000"));
        assert_eq!(release.checksums_url().as_deref(), Some("https://e/sums"));
    }

    #[test]
    fn picks_the_installer_for_this_platform() {
        let release = &parse_releases(SAMPLE, false).unwrap()[0];
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            assert_eq!(release.asset_for(InstallKind::Deb).map(|a| a.url.as_str()), Some("https://e/deb"));
            assert_eq!(release.asset_for(InstallKind::Rpm), None);
            assert_eq!(release.asset_for(InstallKind::Flatpak), None);
            assert_eq!(release.platform_assets().len(), 1);
        }
        assert_eq!(InstallKind::of_asset("paperloom-0.3.0-windows-x86_64-setup.exe"), Some(InstallKind::WindowsSetup));
        assert_eq!(InstallKind::of_asset("paperloom-0.3.0-windows-x86_64.msi"), Some(InstallKind::WindowsMsi));
    }

    #[test]
    fn available_skips_ignored_and_older() {
        let report = CheckReport {
            current: Version::parse("0.2.0").unwrap(),
            releases: parse_releases(SAMPLE, false).unwrap(),
            api_url: String::new(),
            http_status: 200,
            mirror_error: None,
            rate_limit_remaining: None,
            rate_limit_reset: None,
            took: Duration::ZERO,
        };
        assert_eq!(report.available(None).map(|r| r.tag.as_str()), Some("v0.3.0"));
        assert_eq!(report.available(Some("0.3.0")), None);
        assert_eq!(report.newest_newer().map(|r| r.tag.as_str()), Some("v0.3.0"));
    }

    #[test]
    fn reads_sha256sums_lines() {
        let body = "aaaa  other.deb\n0123456789abcdef0123456789abcdef0123456789abcdef0123456789ABCDEF *paperloom.msi\n";
        assert_eq!(parse_checksums(body, "paperloom.msi").as_deref(), Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"));
        assert_eq!(parse_checksums(body, "missing"), None);
    }

    #[test]
    fn prune_keeps_the_new_installer_and_the_one_before() {
        let dir = std::env::temp_dir().join(format!("paperloom-prune-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let names = [
            "paperloom-0.2.0-linux-x86_64.deb",
            "paperloom-0.3.0-linux-x86_64.deb",
            "paperloom-0.3.1-linux-x86_64.deb",
            "paperloom-0.4.0-linux-x86_64.deb",
            "paperloom-0.3.9-linux-x86_64.deb.part",
            "notes.txt",
        ];
        for name in names {
            fs::write(dir.join(name), b"x").unwrap();
        }
        prune_downloads(&dir.join("paperloom-0.4.0-linux-x86_64.deb"));
        let mut left: Vec<String> = fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["notes.txt", "paperloom-0.3.1-linux-x86_64.deb", "paperloom-0.4.0-linux-x86_64.deb"]);
        let _ = fs::remove_dir_all(&dir);
    }
}
