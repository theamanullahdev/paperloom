//! "Use a still of the wallpaper on the lock screen".
//!
//! A video can't play on a lock screen, but a still frame of the current
//! wallpaper can be shown there. When the setting is on, each new wallpaper
//! gets one frame taken (ffmpeg) and handed to the system:
//!
//! - Windows: `Windows.System.UserProfile.LockScreen.SetImageFileAsync`, run
//!   through PowerShell (no admin rights needed).
//! - GNOME-style desktops: the screensaver's `picture-uri` setting.
//! - Cinnamon: its lock screen shows the desktop background, which Paperloom
//!   already replaces with a still of the wallpaper.
//!
//! The result ("ok" or why it failed) is published for the Settings page.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use paperloom_common::Settings;

static LAST: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Called when a wallpaper starts.
pub fn on_play(path: &str) {
    if paperloom_common::load().lock_screen_still {
        apply_in_background(PathBuf::from(path));
    }
}

/// Called on the engine's tick: notices the setting being switched on (or
/// off) and keeps the status line honest.
pub fn tick(settings: &Settings) {
    let mut last = LAST.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if !settings.lock_screen_still {
        last.take();
        crate::player::set_lock_screen_status("off");
        return;
    }
    let current = paperloom_common::EngineState::load().current_wallpaper;
    if let Some(path) = current {
        if last.as_ref() != Some(&path) {
            *last = Some(path.clone());
            drop(last);
            apply_in_background(path);
        }
    }
}

fn apply_in_background(path: PathBuf) {
    *LAST.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(path.clone());
    let _ = std::thread::Builder::new().name("paperloom-lock-screen".into()).spawn(move || {
        let status = match apply(&path) {
            Ok(note) => note,
            Err(why) => {
                paperloom_common::log::warn(&format!("lock screen: {why}"));
                format!("failed: {why}")
            }
        };
        crate::player::set_lock_screen_status(&status);
    });
}

fn apply(video: &Path) -> Result<String, String> {
    let frame = paperloom_common::state_file().with_file_name("lock-screen.jpg");
    let output = paperloom_common::process::command("ffmpeg")
        .args(["-y", "-ss", "1", "-i"])
        .arg(video)
        .args(["-frames:v", "1", "-q:v", "2"])
        .arg(&frame)
        .output()
        .map_err(|err| format!("couldn't run ffmpeg to take a frame ({err})"))?;
    if !output.status.success() || !frame.metadata().is_ok_and(|meta| meta.len() > 0) {
        return Err("couldn't take a still frame from the video".into());
    }
    set_image(&frame)
}

#[cfg(windows)]
fn set_image(image: &Path) -> Result<String, String> {
    let path = image.display().to_string().replace('\'', "''");
    let script = format!(
        "$ErrorActionPreference='Stop'; \
         Add-Type -AssemblyName System.Runtime.WindowsRuntime; \
         $asTask = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {{ $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' }})[0]; \
         $asAction = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {{ $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and !$_.IsGenericMethod -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncAction' }})[0]; \
         [Windows.Storage.StorageFile,Windows.Storage,ContentType=WindowsRuntime] | Out-Null; \
         [Windows.System.UserProfile.LockScreen,Windows.System.UserProfile,ContentType=WindowsRuntime] | Out-Null; \
         $op = [Windows.Storage.StorageFile]::GetFileFromPathAsync('{path}'); \
         $task = $asTask.MakeGenericMethod([Windows.Storage.StorageFile]).Invoke($null, @($op)); $task.Wait(-1) | Out-Null; \
         $file = $task.Result; \
         $set = $asAction.Invoke($null, @([Windows.System.UserProfile.LockScreen]::SetImageFileAsync($file))); $set.Wait(-1) | Out-Null"
    );
    let mut command = std::process::Command::new("powershell");
    paperloom_common::process::hide_console(&mut command);
    let output = command
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .output()
        .map_err(|err| format!("couldn't start PowerShell ({err})"))?;
    if output.status.success() {
        Ok("ok".into())
    } else {
        let text = String::from_utf8_lossy(&output.stderr);
        let line = text.lines().find(|line| !line.trim().is_empty()).unwrap_or("Windows refused the lock screen image");
        Err(format!("Windows refused the lock screen image: {}", line.trim()))
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn set_image(image: &Path) -> Result<String, String> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
    if desktop.contains("cinnamon") {
        return Ok("ok (Cinnamon's lock screen shows the desktop background, which is a still of the wallpaper)".into());
    }
    let schemas = std::process::Command::new("gsettings").arg("list-schemas").output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    if schemas.lines().any(|line| line.trim() == "org.gnome.desktop.screensaver") {
        let uri = format!("file://{}", image.display());
        let mut ok = true;
        for key in ["picture-uri", "picture-uri-dark"] {
            ok &= std::process::Command::new("gsettings").args(["set", "org.gnome.desktop.screensaver", key, &uri]).status().is_ok_and(|status| status.success());
        }
        return if ok { Ok("ok".into()) } else { Err("the desktop refused the lock screen setting".into()) };
    }
    Err(format!("this desktop ({}) has no lock screen image setting Paperloom can change", if desktop.is_empty() { "unknown" } else { &desktop }))
}

#[cfg(target_os = "macos")]
fn set_image(_image: &Path) -> Result<String, String> {
    Err("macOS doesn't let apps change the lock screen picture".into())
}
