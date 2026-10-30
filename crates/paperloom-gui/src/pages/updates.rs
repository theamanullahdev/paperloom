//! Settings → Updates: check GitHub Releases, download the matching
//! installer (resumable, checksum-verified), install it, or skip a version.
//!
//! A workspace: every release on the left (this version, newer, older), the
//! chosen one on the right with its installers and notes, and the technical
//! details (API, checksums, file paths, activity log) on their own tabs.
//! Any version can be downloaded and installed, older ones included.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use iced::widget::{button, column, container, progress_bar, row, text, toggler, Space};
use iced::{Alignment, Background, Border, Element, Length, Task};

use paperloom_common::Settings;
use paperloom_fetch::update::{self, AfterLaunch, CheckReport, InstallKind, Phase, Release, UpdateDownload, Version};

use crate::pages::downloads::{duration, megabytes};
use crate::theme;

enum Check {
    NotYet,
    Running(Instant),
    Done(Box<CheckReport>),
    Failed(String),
}

pub struct State {
    kind: InstallKind,
    kind_reason: String,
    check: Check,
    download: Option<UpdateDownload>,
    /// Smoothed transfer speed: (bytes at last sample, when, bytes/s).
    speed: Option<(u64, Instant, f64)>,
    log: Vec<(u64, String)>,
    /// The package manager is installing the download (password prompt).
    installing: bool,
    /// Installed by this session; restarting switches to it.
    installed: Option<Version>,
    /// The release open on the right, by tag (default: the newest newer one,
    /// else this version).
    selected: Option<String>,
    tab: DetailTab,
    /// Taps on the version number (seven switch developer mode).
    taps: u8,
    /// Why the last install didn't work, shown on the download card.
    install_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailTab {
    Release,
    Details,
    Activity,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// Handled by the app (navigation).
    Back,
    /// Handled by the app: restart into the installed version.
    Restart,
    CheckNow,
    Select(String),
    /// Download only what changed since this installed version.
    QuickDownload(String),
    VersionTapped,
    Tab(DetailTab),
    Checked(Result<Box<CheckReport>, String>),
    Download { version: String, asset: String },
    CancelDownload,
    Install,
    Installed(Result<AfterLaunch, String>),
    OpenFolder,
    OpenUrl(String),
    Ignore(String),
    StopIgnoring,
    CheckOnStartup(bool),
    IncludePrereleases(bool),
    Tick,
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl State {
    pub fn new() -> Self {
        let (kind, kind_reason) = InstallKind::detect_with_reason();
        Self { kind, kind_reason, check: Check::NotYet, download: None, speed: None, log: Vec::new(), installing: false, installed: None, selected: None, tab: DetailTab::Release, taps: 0, install_error: None }
    }

    fn note(&mut self, message: impl Into<String>) {
        self.log.push((now_unix(), message.into()));
        if self.log.len() > 200 {
            self.log.remove(0);
        }
    }

    /// The version this session installed, waiting for a restart.
    pub fn installed(&self) -> Option<&Version> {
        self.installed.as_ref()
    }

    pub fn is_downloading(&self) -> bool {
        self.download.as_ref().is_some_and(UpdateDownload::is_active)
    }

    /// The update to offer (respects "skip this version").
    pub fn available<'a>(&'a self, settings: &Settings) -> Option<&'a Release> {
        match &self.check {
            Check::Done(report) => report.available(settings.updates.ignored_version.as_deref()),
            _ => None,
        }
    }

    /// One line for the Settings page and the sidebar badge.
    pub fn summary(&self, settings: &Settings) -> String {
        match &self.check {
            Check::NotYet => "Not checked yet".into(),
            Check::Running(_) => "Checking for updates…".into(),
            Check::Failed(error) => format!("Couldn't check: {error}"),
            Check::Done(report) => match report.available(settings.updates.ignored_version.as_deref()) {
                Some(release) => format!("Version {} is available", release.version),
                None => match report.newest_newer() {
                    Some(release) => format!("Skipping version {}", release.version),
                    None => "Up to date".into(),
                },
            },
        }
    }

    pub fn start_check(&mut self, settings: &Settings) -> Task<Message> {
        if matches!(self.check, Check::Running(_)) {
            return Task::none();
        }
        self.check = Check::Running(Instant::now());
        self.note(format!("Checking {} (falls back to {})", update::releases_mirror_url(), update::releases_api_url()));
        let include_prereleases = settings.updates.include_prereleases;
        Task::perform(
            crate::pages::online::blocking(move || update::check(include_prereleases).map(Box::new).map_err(|err| err.to_string())),
            Message::Checked,
        )
    }

    pub fn update(&mut self, settings: &mut Settings, message: Message) -> Task<Message> {
        match message {
            Message::Back | Message::Restart | Message::Tick => self.sample_speed(),
            Message::CheckNow => return self.start_check(settings),
            Message::VersionTapped => {
                self.taps += 1;
                if self.taps >= 7 {
                    self.taps = 0;
                    settings.developer_mode = !settings.developer_mode;
                    let _ = paperloom_common::save(settings);
                    self.note(format!("Developer mode {}", if settings.developer_mode { "on" } else { "off" }));
                }
            }
            Message::Select(tag) => {
                self.selected = Some(tag);
                self.tab = DetailTab::Release;
            }
            Message::Tab(tab) => self.tab = tab,
            Message::Checked(Ok(report)) => {
                settings.updates.last_checked = Some(now_unix());
                let _ = paperloom_common::save(settings);
                let found = match report.available(settings.updates.ignored_version.as_deref()) {
                    Some(release) => format!("version {} is available", release.version),
                    None => "no newer version".into(),
                };
                self.note(format!(
                    "HTTP {} in {} ms: {} release(s) listed, {found}",
                    report.http_status,
                    report.took.as_millis(),
                    report.releases.len()
                ));
                self.check = Check::Done(report);
            }
            Message::Checked(Err(error)) => {
                self.note(format!("Check failed: {error}"));
                self.check = Check::Failed(error);
            }
            Message::QuickDownload(version) => {
                let Check::Done(report) = &self.check else { return Task::none() };
                let Some(release) = report.releases.iter().find(|r| r.version.to_string() == version) else { return Task::none() };
                if let Some(old) = self.download.take() {
                    old.cancel();
                }
                self.download = Some(UpdateDownload::start_quick(release));
                self.speed = None;
                self.note(format!("Small update to {version}: downloading only the files that changed"));
            }
            Message::Download { version, asset } => {
                let Check::Done(report) = &self.check else { return Task::none() };
                let release = report.releases.iter().find(|r| r.version.to_string() == version);
                let Some((release, asset)) = release.and_then(|r| r.assets.iter().find(|a| a.name == asset).map(|a| (r, a))) else {
                    return Task::none();
                };
                if let Some(old) = self.download.take() {
                    old.cancel();
                }
                let started = UpdateDownload::start(release, asset);
                let message = format!("Downloading {} from {}", asset.name, asset.url);
                self.download = Some(started);
                self.speed = None;
                self.note(message);
            }
            Message::CancelDownload => {
                if let Some(download) = &self.download {
                    download.cancel();
                    self.note("Download cancelled");
                }
            }
            Message::Install => {
                let Some(download) = &self.download else { return Task::none() };
                if self.installing {
                    return Task::none();
                }
                let path = download.snapshot().path;
                self.installing = true;
                self.install_error = None;
                self.note(if cfg!(target_os = "linux") {
                    format!("Installing {} — your system asks for your password", path.display())
                } else {
                    format!("Opening installer {}", path.display())
                });
                return Task::perform(crate::pages::online::blocking(move || update::launch_installer(&path)), Message::Installed);
            }
            Message::Installed(result) => {
                self.installing = false;
                match result {
                    Ok(AfterLaunch::ExitApp) => {
                        // The installer replaces both executables: let it.
                        crate::engine_client::quit();
                        std::process::exit(0);
                    }
                    Ok(AfterLaunch::Updated) => {
                        // The new files are in place: start the new version (and its helpers) and leave.
                        self.note("Update put in place: restarting Paperloom");
                        crate::engine_client::restart_app();
                    }
                    Ok(AfterLaunch::KeepRunning) => self.note("Installer opened — finish there, then restart Paperloom"),
                    Ok(AfterLaunch::Installed) => {
                        let version = self.download.as_ref().map(|download| download.snapshot().version);
                        self.note(format!("Installed{} — restart Paperloom to use it", version.as_ref().map(|v| format!(" version {v}")).unwrap_or_default()));
                        self.installed = version;
                    }
                    Err(error) => {
                        self.note(format!("Not installed: {error}"));
                        self.install_error = Some(error);
                    }
                }
            }
            Message::OpenFolder => {
                let dir = paperloom_common::updates_dir();
                let _ = std::fs::create_dir_all(&dir);
                if let Err(err) = update::open_url(&dir.display().to_string()) {
                    self.note(format!("Couldn't open {}: {err}", dir.display()));
                }
            }
            Message::OpenUrl(url) => {
                if let Err(err) = update::open_url(&url) {
                    self.note(format!("Couldn't open {url}: {err}"));
                }
            }
            Message::Ignore(version) => {
                self.note(format!("Skipping version {version}"));
                settings.updates.ignored_version = Some(version);
                let _ = paperloom_common::save(settings);
            }
            Message::StopIgnoring => {
                self.note("No longer skipping any version");
                settings.updates.ignored_version = None;
                let _ = paperloom_common::save(settings);
            }
            Message::CheckOnStartup(value) => {
                settings.updates.check_on_startup = value;
                let _ = paperloom_common::save(settings);
            }
            Message::IncludePrereleases(value) => {
                settings.updates.include_prereleases = value;
                let _ = paperloom_common::save(settings);
                return self.start_check(settings);
            }
        }
        Task::none()
    }

    fn sample_speed(&mut self) {
        let Some(download) = &self.download else { return };
        let snapshot = download.snapshot();
        let now = Instant::now();
        self.speed = Some(match self.speed {
            Some((bytes, at, rate)) => {
                let elapsed = now.duration_since(at).as_secs_f64();
                if elapsed < 0.2 {
                    (bytes, at, rate)
                } else {
                    let instant = snapshot.bytes_done.saturating_sub(bytes) as f64 / elapsed;
                    (snapshot.bytes_done, now, if rate == 0.0 { instant } else { rate * 0.6 + instant * 0.4 })
                }
            }
            None => (snapshot.bytes_done, now, 0.0),
        });
    }

    pub fn header<'a>(&'a self) -> Element<'a, Message> {
        button(crate::ui::label("Back to Settings")).style(crate::ui::secondary).on_press(Message::Back).into()
    }

    pub fn body<'a>(&'a self, settings: &'a Settings, t: theme::Tokens) -> Element<'a, Message> {
        let checking = matches!(self.check, Check::Running(_));
        let (status, color) = match &self.check {
            Check::NotYet => ("Not checked yet this session".to_string(), t.muted_foreground),
            Check::Running(since) => (format!("Checking… ({:.1}s)", since.elapsed().as_secs_f32()), t.primary),
            Check::Failed(error) => (format!("Check failed: {error}"), t.destructive),
            Check::Done(_) => (self.summary(settings), t.success),
        };
        let top = card(
            row![
                column![
                    iced::widget::mouse_area(text(format!("Paperloom {}", Version::current())).font(theme::DISPLAY_FONT).size(18).color(t.foreground)).on_press(Message::VersionTapped),
                    text(status).size(13).color(color),
                ]
                .spacing(2),
                Space::new().width(Length::Fill),
                button(crate::ui::label(if checking { "Checking…" } else { "Check now" })).on_press_maybe((!checking).then_some(Message::CheckNow)),
            ]
            .align_y(Alignment::Center)
            .into(),
            t,
        );

        let Check::Done(report) = &self.check else {
            // Nothing to list yet: the same details and activity, stacked.
            let mut page = column![top, self.this_copy(t), self.check_card(settings, t)].spacing(12);
            if let Some(download) = &self.download {
                page = page.push(self.download_card(download, t));
            }
            page = page.push(self.log_card(t));
            return crate::ui::scroll_page(page).height(Length::Fill).into();
        };

        let selected = self.selected_release(report, settings);
        let list = {
            let mut items = column![].spacing(6);
            for release in &report.releases {
                items = items.push(self.release_row(release, report, settings, selected.is_some_and(|chosen| chosen.tag == release.tag), t));
            }
            if report.releases.is_empty() {
                items = items.push(text("No releases published yet.").size(12).color(t.muted_foreground));
            }
            container(iced::widget::scrollable(items.padding([0, 8])).height(Length::Fill).direction(iced::widget::scrollable::Direction::Vertical(
                iced::widget::scrollable::Scrollbar::hidden(),
            )))
            .width(Length::Fixed(250.0))
            .height(Length::Fill)
        };

        let tabs = crate::ui::segmented(
            vec![
                ("Release".to_string(), self.tab == DetailTab::Release, Message::Tab(DetailTab::Release)),
                ("Details".to_string(), self.tab == DetailTab::Details, Message::Tab(DetailTab::Details)),
                ("Activity".to_string(), self.tab == DetailTab::Activity, Message::Tab(DetailTab::Activity)),
            ],
            t,
        );
        let mut pane = column![tabs].spacing(12);
        if let Some(download) = &self.download {
            pane = pane.push(self.download_card(download, t));
        }
        pane = pane.push(match self.tab {
            DetailTab::Release => match selected {
                Some(release) => self.release_card(release, report, settings, t),
                None => text("No release selected.").color(t.muted_foreground).into(),
            },
            DetailTab::Details => column![self.this_copy(t), self.check_card(settings, t)].spacing(12).into(),
            DetailTab::Activity => self.log_card(t),
        });
        let detail = iced::widget::scrollable(pane.padding([0, 8])).width(Length::Fill).height(Length::Fill).direction(
            iced::widget::scrollable::Direction::Vertical(iced::widget::scrollable::Scrollbar::hidden()),
        );

        column![top, row![list, detail].spacing(12).height(Length::Fill)].spacing(12).height(Length::Fill).into()
    }

    /// The release shown on the right.
    fn selected_release<'a>(&'a self, report: &'a CheckReport, settings: &Settings) -> Option<&'a Release> {
        if let Some(tag) = &self.selected {
            if let Some(release) = report.releases.iter().find(|release| &release.tag == tag) {
                return Some(release);
            }
        }
        report.available(settings.updates.ignored_version.as_deref()).or_else(|| report.releases.iter().find(|release| release.version == report.current)).or(report.releases.first())
    }

    /// One line in the list: tag, date and how it relates to this copy.
    fn release_row<'a>(&'a self, release: &'a Release, report: &'a CheckReport, settings: &Settings, active: bool, t: theme::Tokens) -> Element<'a, Message> {
        let (relation, color) = match release.version.cmp(&report.current) {
            std::cmp::Ordering::Greater if settings.updates.ignored_version.as_deref().and_then(Version::parse).as_ref() == Some(&release.version) => ("skipped", t.muted_foreground),
            std::cmp::Ordering::Greater => ("newer", t.primary),
            std::cmp::Ordering::Equal => ("installed", t.success),
            std::cmp::Ordering::Less => ("older", t.muted_foreground),
        };
        let date = release.published_at.split('T').next().unwrap_or_default().to_string();
        let body = column![
            row![text(release.tag.clone()).size(14).color(t.foreground), Space::new().width(Length::Fill), text(relation).size(11).color(color)].align_y(Alignment::Center),
            text(format!("{date}{}", if release.prerelease { " · pre-release" } else { "" })).size(11).color(t.muted_foreground),
        ]
        .spacing(2);
        button(container(body).padding([8, 10]).width(Length::Fill))
            .padding(0)
            .width(Length::Fill)
            .style(move |_theme, _status| button::Style {
                background: Some(Background::Color(if active { t.muted } else { t.surface })),
                text_color: t.foreground,
                border: Border { color: if active { t.primary } else { t.border }, width: 1.0, radius: 4.0.into() },
                ..button::Style::default()
            })
            .on_press(Message::Select(release.tag.clone()))
            .into()
    }

    fn this_copy<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "unknown".into());
        card(
            column![
                heading("This copy", t),
                detail("Version", Version::current().to_string(), t),
                detail("Platform", format!("{} / {}", std::env::consts::OS, std::env::consts::ARCH), t),
                detail("Installed as", format!("{} — {}", self.kind.label(), self.kind_reason), t),
                detail("Executable", exe, t),
                detail("Update files", paperloom_common::updates_dir().display().to_string(), t),
                detail("Release list", update::releases_mirror_url(), t),
                detail("Fallback", update::releases_api_url(), t),
            ]
            .spacing(3)
            .into(),
            t,
        )
    }

    fn check_card<'a>(&'a self, settings: &'a Settings, t: theme::Tokens) -> Element<'a, Message> {
        let (status, color) = match &self.check {
            Check::NotYet => ("Not checked yet this session".to_string(), t.muted_foreground),
            Check::Running(since) => (format!("Checking… ({:.1}s)", since.elapsed().as_secs_f32()), t.primary),
            Check::Failed(error) => (format!("Check failed: {error}"), t.destructive),
            Check::Done(_) => (self.summary(settings), t.success),
        };
        let checking = matches!(self.check, Check::Running(_));

        let mut details = column![].spacing(3);
        details = details.push(detail("Last checked", settings.updates.last_checked.map(format_unix).unwrap_or_else(|| "never".into()), t));
        if let Check::Done(report) = &self.check {
            details = details.push(detail("Answered by", report.api_url.clone(), t));
            if let Some(error) = &report.mirror_error {
                details = details.push(detail("Mirror failed", error.clone(), t));
            }
            details = details.push(detail("HTTP status", report.http_status.to_string(), t));
            details = details.push(detail("Took", format!("{} ms", report.took.as_millis()), t));
            details = details.push(detail(
                "API quota left",
                match (&report.rate_limit_remaining, report.rate_limit_reset) {
                    (Some(left), Some(reset)) => format!("{left} requests (resets {})", format_unix(reset)),
                    (Some(left), None) => format!("{left} requests"),
                    _ => "not used (static release list)".into(),
                },
                t,
            ));
        }
        if let Some(version) = &settings.updates.ignored_version {
            details = details.push(
                row![
                    detail("Skipping", format!("version {version}"), t),
                    button(crate::ui::label("Stop skipping").size(12)).style(crate::ui::secondary).padding([4, 10]).on_press(Message::StopIgnoring),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            );
        }

        card(
            column![
                row![
                    heading("Check for updates", t),
                    Space::new().width(Length::Fill),
                    button(crate::ui::label(if checking { "Checking…" } else { "Check now" })).on_press_maybe((!checking).then_some(Message::CheckNow)),
                ]
                .align_y(Alignment::Center),
                text(status).size(14).color(color),
                details,
                toggler(settings.updates.check_on_startup).label("Check automatically when Paperloom starts").on_toggle(Message::CheckOnStartup),
                toggler(settings.updates.include_prereleases).label("Include pre-releases").on_toggle(Message::IncludePrereleases),
            ]
            .spacing(10)
            .into(),
            t,
        )
    }

    fn release_card<'a>(&'a self, release: &'a Release, report: &'a CheckReport, settings: &'a Settings, t: theme::Tokens) -> Element<'a, Message> {
        let version = release.version.to_string();
        let ignored = settings.updates.ignored_version.as_deref().and_then(Version::parse).as_ref() == Some(&release.version);
        let preferred = release.asset_for(self.kind).map(|asset| asset.name.clone());

        let mut actions = row![button(crate::ui::label("Release page")).style(crate::ui::secondary).on_press(Message::OpenUrl(release.page_url.clone()))].spacing(8);
        if release.version > report.current && !ignored {
            actions = actions.push(button(crate::ui::label("Skip this version")).style(crate::ui::secondary).on_press(Message::Ignore(version.clone())));
        }

        let mut body = column![
            row![
                heading(&release.title, t),
                Space::new().width(Length::Fill),
                text(match release.version.cmp(&report.current) {
                    std::cmp::Ordering::Greater => "New version",
                    std::cmp::Ordering::Equal => "Installed version",
                    std::cmp::Ordering::Less => "Older version",
                })
                .size(12)
                .color(t.primary),
            ]
            .align_y(Alignment::Center),
            detail("Version", format!("{} (you have {})", release.version, Version::current()), t),
            detail("Tag", release.tag.clone(), t),
            detail("Published", release.published_at.replace('T', " ").replace('Z', " UTC"), t),
            detail("Page", release.page_url.clone(), t),
            actions,
        ]
        .spacing(6);

        if !self.kind.self_updates() {
            body = body.push(
                text("This copy is a Flatpak, so Flatpak updates it: use your Software Manager, or run `flatpak update`.")
                    .size(13)
                    .color(t.muted_foreground),
            );
        } else {
            if release.version < report.current {
                body = body.push(text("An older version: installing it replaces the one you have now. Your library and settings are kept.").size(12).color(t.muted_foreground));
            }
            // Only what changed: usually a fraction of the installer.
            if release.version != report.current && paperloom_fetch::delta::offered(release) {
                body = body.push(
                    container(
                        row![
                            column![
                                text("Small update").size(14).color(t.foreground),
                                text("Downloads only the files that changed since your version, usually a small part of the installer. Everything is checked before it is used.").size(12).color(t.muted_foreground),
                            ]
                            .spacing(2)
                            .width(Length::Fill),
                            button(crate::ui::label("Download").size(12)).padding([4, 10]).on_press_maybe((!self.is_downloading()).then(|| Message::QuickDownload(version.clone()))),
                        ]
                        .spacing(12)
                        .align_y(Alignment::Center),
                    )
                    .padding(10)
                    .width(Length::Fill)
                    .style(move |_theme| container::Style { border: Border { color: t.primary, width: 1.0, radius: 4.0.into() }, ..container::Style::default() }),
                );
            }
            body = body.push(text("Installers for this computer").size(13).color(t.muted_foreground));
            let assets = release.platform_assets();
            if assets.is_empty() {
                body = body.push(text("This release has no installer for this platform.").size(13).color(t.destructive));
            }
            for asset in assets {
                let matches = preferred.as_deref() == Some(asset.name.as_str());
                body = body.push(asset_row(&version, asset, matches, self.is_downloading(), t));
            }
        }

        if !release.notes.trim().is_empty() {
            body = body.push(text("Release notes").size(13).color(t.muted_foreground));
            body = body.push(container(text(release.notes.clone()).size(13).color(t.foreground)).padding(10).width(Length::Fill).style(move |_theme| {
                container::Style { background: Some(Background::Color(t.muted)), border: Border { radius: 4.0.into(), ..Border::default() }, ..container::Style::default() }
            }));
        }
        card(body.into(), t)
    }

    fn download_card<'a>(&'a self, download: &UpdateDownload, t: theme::Tokens) -> Element<'a, Message> {
        let s = download.snapshot();
        let (label, color) = match s.phase {
            Phase::Downloading => ("Downloading", t.primary),
            Phase::Retrying => ("Will retry", t.primary_hover),
            Phase::Verifying => ("Verifying checksum", t.primary),
            Phase::Ready if self.installed.as_ref() == Some(&s.version) => ("Installed — restart to use it", t.success),
            Phase::Ready if self.installing => ("Installing…", t.primary),
            Phase::Ready => ("Ready to install", t.success),
            Phase::Failed => ("Failed", t.destructive),
            Phase::Cancelled => ("Cancelled", t.muted_foreground),
        };
        let fraction = match s.bytes_total {
            Some(total) if total > 0 => (s.bytes_done as f32 / total as f32).clamp(0.0, 1.0),
            _ if s.phase == Phase::Ready => 1.0,
            _ => 0.0,
        };
        let bar = progress_bar(0.0..=1.0, fraction).girth(6).style(move |_theme| progress_bar::Style {
            background: Background::Color(t.muted),
            bar: Background::Color(if s.phase == Phase::Failed { t.destructive } else { t.primary }),
            border: Border { radius: 3.0.into(), ..Border::default() },
        });

        let rate = self.speed.map(|(_, _, rate)| rate).unwrap_or(0.0);
        let mut progress = match s.bytes_total {
            Some(total) if total > 0 => format!("{:.0}%   {} of {}", fraction * 100.0, megabytes(s.bytes_done), megabytes(total)),
            _ => megabytes(s.bytes_done),
        };
        if s.phase == Phase::Downloading && rate > 1.0 {
            progress.push_str(&format!("   {}/s", megabytes(rate as u64)));
            if let Some(total) = s.bytes_total {
                progress.push_str(&format!("   {} left", duration(total.saturating_sub(s.bytes_done) as f64 / rate)));
            }
        }
        let elapsed = s.finished.unwrap_or_else(Instant::now).duration_since(s.started);
        progress.push_str(&format!("   elapsed {}", duration(elapsed.as_secs_f64())));

        let none = || "—".to_string();
        let mut details = column![
            detail("Version", s.version.to_string(), t),
            detail("Download URL", s.asset.url.clone(), t),
            detail("Saves to", s.path.display().to_string(), t),
            detail("Partial file", s.part_path.display().to_string(), t),
            detail("Attempts", s.attempts.to_string(), t),
            detail("Checksum from", s.checksum_source.map(str::to_string).unwrap_or_else(|| "none published".into()), t),
            detail("Expected SHA-256", s.expected_sha256.clone().unwrap_or_else(none), t),
            detail("Actual SHA-256", s.actual_sha256.clone().unwrap_or_else(none), t),
        ]
        .spacing(2);
        if let Some(message) = &s.message {
            details = details.push(detail("Last message", message.clone(), t));
        }
        if let Some(error) = &self.install_error {
            details = details.push(text(format!("The install did not finish: {error}")).size(12).color(t.destructive));
        }

        let small = |label: &'static str, message: Message| button(crate::ui::label(label).size(12)).style(crate::ui::secondary).padding([4, 10]).on_press(message);
        let mut actions = column![].spacing(6).align_x(Alignment::End);
        match s.phase {
            Phase::Ready if self.installed.as_ref() == Some(&s.version) => {
                actions = actions.push(button(crate::ui::label("Restart Paperloom").size(12)).padding([4, 10]).on_press(Message::Restart));
                actions = actions.push(small("Show in folder", Message::OpenFolder));
            }
            Phase::Ready => {
                let label = match (self.installing, cfg!(windows)) {
                    (true, _) => "Installing…",
                    (false, true) => "Install and restart",
                    (false, false) => "Install",
                };
                actions = actions.push(button(crate::ui::label(label).size(12)).padding([4, 10]).on_press_maybe((!self.installing).then_some(Message::Install)));
                actions = actions.push(small("Show in folder", Message::OpenFolder));
            }
            Phase::Downloading | Phase::Retrying | Phase::Verifying => actions = actions.push(small("Cancel", Message::CancelDownload)),
            Phase::Failed | Phase::Cancelled => {
                actions = actions.push(small(
                    "Try again",
                    if s.asset.name.ends_with(paperloom_fetch::delta::MARKER_SUFFIX) {
                        Message::QuickDownload(s.version.to_string())
                    } else {
                        Message::Download { version: s.version.to_string(), asset: s.asset.name.clone() }
                    },
                ));
            }
        }

        card(
            row![
                column![
                    row![text(s.asset.name.clone()).size(14).color(t.foreground), Space::new().width(Length::Fill), text(label).size(12).color(color)]
                        .align_y(Alignment::Center),
                    bar,
                    text(progress).size(12).color(t.muted_foreground),
                    details,
                ]
                .spacing(6)
                .width(Length::Fill),
                actions,
            ]
            .spacing(12)
            .into(),
            t,
        )
    }

    fn log_card<'a>(&'a self, t: theme::Tokens) -> Element<'a, Message> {
        let mut list = column![heading("Activity", t)].spacing(3);
        if self.log.is_empty() {
            list = list.push(text("Nothing yet.").size(12).color(t.muted_foreground));
        }
        for (at, message) in self.log.iter().rev() {
            list = list.push(
                row![text(format_unix(*at)).size(11).color(t.muted_foreground).width(Length::Fixed(150.0)), text(message.clone()).size(11).color(t.foreground)]
                    .wrap(),
            );
        }
        card(list.into(), t)
    }
}

fn asset_row<'a>(version: &str, asset: &'a update::Asset, matches_install: bool, busy: bool, t: theme::Tokens) -> Element<'a, Message> {
    let download = Message::Download { version: version.to_string(), asset: asset.name.clone() };
    let mut button = button(crate::ui::label("Download").size(12)).padding([4, 10]);
    if !matches_install {
        button = button.style(crate::ui::secondary);
    }
    let mut title = row![text(asset.name.clone()).size(13).color(t.foreground)].spacing(8).align_y(Alignment::Center);
    if matches_install {
        title = title.push(text("matches this install").size(11).color(t.success));
    }
    container(
        row![
            column![
                title,
                detail("Size", megabytes(asset.size), t),
                detail("Type", if asset.content_type.is_empty() { "—".into() } else { asset.content_type.clone() }, t),
                detail("Downloads", asset.download_count.to_string(), t),
                detail("SHA-256", asset.sha256.clone().unwrap_or_else(|| "from SHA256SUMS.txt (if published)".into()), t),
                detail("URL", asset.url.clone(), t),
            ]
            .spacing(2)
            .width(Length::Fill),
            button.on_press_maybe((!busy).then_some(download)),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .padding(10)
    .width(Length::Fill)
    .style(move |_theme| container::Style {
        border: Border { color: if matches_install { t.primary } else { t.border }, width: 1.0, radius: 4.0.into() },
        ..container::Style::default()
    })
    .into()
}

fn heading<'a>(label: &str, t: theme::Tokens) -> Element<'a, Message> {
    text(label.to_string()).font(theme::BODY_FONT).size(15).color(t.foreground).into()
}

fn detail<'a>(label: &'static str, value: String, t: theme::Tokens) -> Element<'a, Message> {
    row![text(label).size(11).color(t.muted_foreground).width(Length::Fixed(120.0)), text(value).size(11).color(t.foreground)].wrap().into()
}

fn card<'a>(content: Element<'a, Message>, t: theme::Tokens) -> Element<'a, Message> {
    container(content)
        .padding(12)
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(t.surface)),
            border: Border { color: t.border, width: 1.0, radius: 6.0.into() },
            ..container::Style::default()
        })
        .into()
}

/// Unix seconds as "YYYY-MM-DD HH:MM UTC" (no date crate for one label).
fn format_unix(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let (hour, minute) = ((secs % 86_400) / 3600, (secs % 3600) / 60);
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02} UTC")
}

#[cfg(test)]
mod tests {
    use super::format_unix;

    #[test]
    fn formats_unix_time() {
        assert_eq!(format_unix(0), "1970-01-01 00:00 UTC");
        assert_eq!(format_unix(1_792_281_600), "2026-10-18 00:00 UTC");
    }
}
