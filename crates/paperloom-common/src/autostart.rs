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

/// What gets started at login: the wallpaper engine (restores the last
/// wallpaper) or the widgets process (draws the placed widgets).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Program {
    Engine,
    Widgets,
}

impl Program {
    fn binary(self) -> &'static str {
        match self {
            Program::Engine => "paperloom-engine",
            Program::Widgets => "paperloom-widgets",
        }
    }

    /// The name of the login entry (file stem / label suffix).
    #[cfg_attr(windows, allow(dead_code))]
    fn entry(self) -> &'static str {
        self.binary()
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    fn registry_value(self) -> &'static str {
        match self {
            Program::Engine => "Paperloom",
            Program::Widgets => "PaperloomWidgets",
        }
    }

    #[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
    fn title(self) -> (&'static str, &'static str) {
        match self {
            Program::Engine => ("Paperloom wallpaper", "Restores your live wallpaper at login"),
            Program::Widgets => ("Paperloom widgets", "Shows your desktop widgets at login"),
        }
    }
}

fn program_path(program: Program) -> io::Result<PathBuf> {
    crate::process::sibling_binary(program.binary())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{} isn't installed next to this app", program.binary())))
}

/// Whether the wallpaper engine starts at login.
pub fn is_enabled() -> bool {
    imp::is_enabled(Program::Engine)
}

pub fn set_enabled(enabled: bool) -> io::Result<()> {
    set_program(Program::Engine, enabled)
}

pub fn is_program_enabled(program: Program) -> bool {
    imp::is_enabled(program)
}

pub fn set_program(program: Program, enabled: bool) -> io::Result<()> {
    if enabled {
        imp::enable(program)
    } else {
        imp::disable(program)
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use std::{fs, io, path::PathBuf};

    /// Always the *host* `~/.config/autostart`: inside a Flatpak
    /// `XDG_CONFIG_HOME` points into the sandbox, which the session never
    /// reads (the manifest grants `xdg-config/autostart:create` for this).
    use super::Program;

    fn entry_file(program: Program) -> PathBuf {
        let home = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()).unwrap_or_default();
        home.join(format!(".config/autostart/{}.desktop", program.entry()))
    }

    fn exec_line(program: Program) -> io::Result<String> {
        if let Ok(app_id) = std::env::var("FLATPAK_ID") {
            return Ok(format!("flatpak run --command={} {app_id}", program.binary()));
        }
        let path = super::program_path(program)?;
        Ok(format!("\"{}\"", path.display()))
    }

    pub fn is_enabled(program: Program) -> bool {
        entry_file(program).is_file()
    }

    pub fn enable(program: Program) -> io::Result<()> {
        let file = entry_file(program);
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        let (name, comment) = program.title();
        fs::write(
            file,
            format!(
                "[Desktop Entry]\nType=Application\nName={name}\nComment={comment}\nExec={}\nIcon=paperloom\nTerminal=false\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n",
                exec_line(program)?
            ),
        )
    }

    pub fn disable(program: Program) -> io::Result<()> {
        match fs::remove_file(entry_file(program)) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::{fs, io, path::PathBuf};

    use super::Program;

    fn label(program: Program) -> String {
        match program {
            Program::Engine => "io.github.theamanullahdev.paperloom.engine".to_string(),
            Program::Widgets => "io.github.theamanullahdev.paperloom.widgets".to_string(),
        }
    }

    fn agent_file(program: Program) -> PathBuf {
        let home = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()).unwrap_or_default();
        home.join("Library/LaunchAgents").join(format!("{}.plist", label(program)))
    }

    fn xml_escape(value: &str) -> String {
        value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
    }

    pub fn is_enabled(program: Program) -> bool {
        agent_file(program).is_file()
    }

    pub fn enable(program: Program) -> io::Result<()> {
        let engine = super::program_path(program)?;
        let label = label(program);
        let file = agent_file(program);
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
    <string>{label}</string>
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

    pub fn disable(program: Program) -> io::Result<()> {
        match fs::remove_file(agent_file(program)) {
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

    use super::Program;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    pub fn is_enabled(program: Program) -> bool {
        let (key, value) = (wide(RUN_KEY), wide(program.registry_value()));
        // SAFETY: valid NUL-terminated strings; null data pointer = existence/size query.
        let status = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
        };
        status == ERROR_SUCCESS
    }

    pub fn enable(program: Program) -> io::Result<()> {
        let command = wide(&format!("\"{}\"", super::program_path(program)?.display()));
        let (key, value) = (wide(RUN_KEY), wide(program.registry_value()));
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

    pub fn disable(program: Program) -> io::Result<()> {
        let (key, value) = (wide(RUN_KEY), wide(program.registry_value()));
        // SAFETY: valid NUL-terminated strings.
        let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr()) };
        if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(status as i32))
        }
    }
}
