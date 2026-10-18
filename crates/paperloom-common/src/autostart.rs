//! "Start Paperloom when I log in": registers `paperloom-engine` with the
//! OS's own login-items mechanism. The engine then restores the last
//! wallpaper by itself (`EngineState`), so the GUI never has to run at
//! login — only the renderer, which is the piece that has to be there.
//!
//! - Linux: an XDG autostart entry (`~/.config/autostart`), which Cinnamon,
//!   GNOME, KDE and the rest all honour.
//! - Windows: the per-user `Run` registry key.
//! - macOS: a per-user LaunchAgent with `RunAtLoad`.

use std::io;
use std::path::PathBuf;

const ENGINE: &str = "paperloom-engine";

fn engine_path() -> io::Result<PathBuf> {
    crate::process::sibling_binary(ENGINE)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "paperloom-engine isn't installed next to this app"))
}

pub fn is_enabled() -> bool {
    imp::is_enabled()
}

pub fn set_enabled(enabled: bool) -> io::Result<()> {
    if enabled {
        imp::enable()
    } else {
        imp::disable()
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use std::{fs, io, path::PathBuf};

    /// Always the *host* `~/.config/autostart`: inside a Flatpak
    /// `XDG_CONFIG_HOME` points into the sandbox, which the session never
    /// reads (the manifest grants `xdg-config/autostart:create` for this).
    fn entry_file() -> PathBuf {
        let home = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()).unwrap_or_default();
        home.join(".config/autostart/paperloom-engine.desktop")
    }

    fn exec_line() -> io::Result<String> {
        if let Ok(app_id) = std::env::var("FLATPAK_ID") {
            return Ok(format!("flatpak run --command={} {app_id}", super::ENGINE));
        }
        let path = super::engine_path()?;
        Ok(format!("\"{}\"", path.display()))
    }

    pub fn is_enabled() -> bool {
        entry_file().is_file()
    }

    pub fn enable() -> io::Result<()> {
        let file = entry_file();
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(
            file,
            format!(
                "[Desktop Entry]\nType=Application\nName=Paperloom wallpaper\nComment=Restores your live wallpaper at login\nExec={}\nIcon=paperloom\nTerminal=false\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n",
                exec_line()?
            ),
        )
    }

    pub fn disable() -> io::Result<()> {
        match fs::remove_file(entry_file()) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::{fs, io, path::PathBuf};

    const LABEL: &str = "io.github.theamanullahdev.paperloom.engine";

    fn agent_file() -> PathBuf {
        let home = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()).unwrap_or_default();
        home.join("Library/LaunchAgents").join(format!("{LABEL}.plist"))
    }

    fn xml_escape(value: &str) -> String {
        value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
    }

    pub fn is_enabled() -> bool {
        agent_file().is_file()
    }

    pub fn enable() -> io::Result<()> {
        let engine = super::engine_path()?;
        let file = agent_file();
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(
            file,
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#,
                xml_escape(&engine.display().to_string())
            ),
        )
    }

    pub fn disable() -> io::Result<()> {
        match fs::remove_file(agent_file()) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::io;
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
    };

    fn wide(value: &str) -> Vec<u16> {
        std::ffi::OsStr::new(value).encode_wide().chain(Some(0)).collect()
    }

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "Paperloom";

    pub fn is_enabled() -> bool {
        let (key, value) = (wide(RUN_KEY), wide(VALUE));
        // SAFETY: valid NUL-terminated strings; null data pointer = existence/size query.
        let status = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
        };
        status == ERROR_SUCCESS
    }

    pub fn enable() -> io::Result<()> {
        let command = wide(&format!("\"{}\"", super::engine_path()?.display()));
        let (key, value) = (wide(RUN_KEY), wide(VALUE));
        // SAFETY: valid NUL-terminated strings; the size covers the trailing NUL.
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                REG_SZ,
                command.as_ptr().cast(),
                (command.len() * 2) as u32,
            )
        };
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(status as i32))
        }
    }

    pub fn disable() -> io::Result<()> {
        let (key, value) = (wide(RUN_KEY), wide(VALUE));
        // SAFETY: valid NUL-terminated strings.
        let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr()) };
        if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(status as i32))
        }
    }
}
