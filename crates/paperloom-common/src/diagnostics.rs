//! One block of plain text that says what this installation looks like and
//! what went wrong lately — so "it doesn't work" turns into something that
//! can be pasted and read. API keys are removed.

use std::process::Command;

use crate::{log, EngineState, Settings};

/// `extra`: sections the caller knows better (install kind, memory, library
/// size…) as (heading, lines).
pub fn gather(extra: &[(String, Vec<String>)]) -> String {
    let settings = crate::load();
    let mut out = String::new();
    let mut section = |title: &str, lines: Vec<String>| {
        out.push_str(&format!("== {title} ==\n"));
        for line in lines {
            out.push_str(&line);
            out.push('\n');
        }
        out.push('\n');
    };

    section(
        "Paperloom",
        vec![
            format!("version: {}", env!("CARGO_PKG_VERSION")),
            format!("time: {} UTC", log::format_unix(log::now_unix())),
            format!("executable: {}", std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default()),
            format!("developer mode: {}", settings.developer_mode),
        ],
    );
    section("System", system_lines());
    for (title, lines) in extra {
        section(title, lines.clone());
    }
    section("Helpers", helper_lines());
    section("Settings (keys removed)", redacted(&settings).lines().map(str::to_string).collect());
    section("Engine state", state_lines());
    for (name, title) in [("engine", "Engine log (last 40 lines)"), ("gui", "App log (last 40 lines)"), ("crash", "Crash log (last 40 lines)")] {
        let tail = log::tail(name, 40);
        section(title, if tail.is_empty() { vec!["(empty)".into()] } else { tail.lines().map(str::to_string).collect() });
    }
    let start_failure = std::fs::read_to_string(crate::engine_log()).unwrap_or_default();
    section("Last engine start failure", if start_failure.trim().is_empty() { vec!["(none)".into()] } else { start_failure.lines().map(str::to_string).collect() });
    out
}

fn run(program: &str, args: &[&str]) -> Option<String> {
    let mut command = Command::new(program);
    command.args(args);
    crate::process::hide_console(&mut command);
    let output = command.output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_string()).filter(|text| !text.is_empty())
}

fn system_lines() -> Vec<String> {
    let mut lines = vec![format!("os: {} / {}", std::env::consts::OS, std::env::consts::ARCH)];
    match std::env::consts::OS {
        "linux" => {
            if let Ok(release) = std::fs::read_to_string("/etc/os-release") {
                if let Some(name) = release.lines().find_map(|line| line.strip_prefix("PRETTY_NAME=")) {
                    lines.push(format!("distribution: {}", name.trim_matches('"')));
                }
            }
            lines.extend(run("uname", &["-r"]).map(|kernel| format!("kernel: {kernel}")));
            for key in ["XDG_SESSION_TYPE", "XDG_CURRENT_DESKTOP", "WAYLAND_DISPLAY", "DISPLAY", "FLATPAK_ID"] {
                lines.push(format!("{key}: {}", std::env::var(key).unwrap_or_else(|_| "-".into())));
            }
            if let Some(gpu) = run("sh", &["-c", "lspci 2>/dev/null | grep -iE 'vga|3d|display' | head -3"]) {
                lines.extend(gpu.lines().map(|line| format!("gpu: {line}")));
            }
            if let Some(monitors) = run("sh", &["-c", "xrandr --listmonitors 2>/dev/null | tail -n +2"]) {
                lines.extend(monitors.lines().map(|line| format!("monitor:{line}")));
            }
        }
        "windows" => {
            lines.extend(run("cmd", &["/C", "ver"]).map(|version| format!("version: {version}")));
            if let Some(gpu) = run("powershell", &["-NoProfile", "-Command", "(Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name) -join '; '"]) {
                lines.push(format!("gpu: {gpu}"));
            }
            if let Some(cpu) = run("powershell", &["-NoProfile", "-Command", "(Get-CimInstance Win32_Processor | Select-Object -First 1 -ExpandProperty Name)"]) {
                lines.push(format!("cpu: {cpu}"));
            }
        }
        "macos" => {
            lines.extend(run("sw_vers", &["-productVersion"]).map(|version| format!("version: {version}")));
        }
        _ => {}
    }
    lines
}

fn helper_lines() -> Vec<String> {
    let folder = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|p| p.to_path_buf()));
    let mut lines = Vec::new();
    for name in ["paperloom-engine", "paperloom-gui", "ffmpeg", "libmpv-2.dll", "vulkan-1.dll"] {
        let file = if name.contains('.') { name.to_string() } else { crate::process::exe_name(name) };
        if name.ends_with(".dll") && !cfg!(windows) {
            continue;
        }
        let found = folder.as_ref().map(|dir| dir.join(&file)).filter(|path| path.is_file());
        lines.push(format!("{file}: {}", found.map_or("not next to the app".to_string(), |path| path.display().to_string())));
    }
    let ffmpeg = crate::process::command("ffmpeg").arg("-version").output().ok().and_then(|o| String::from_utf8_lossy(&o.stdout).lines().next().map(str::to_string));
    lines.push(format!("ffmpeg version: {}", ffmpeg.unwrap_or_else(|| "not found".into())));
    lines
}

fn redacted(settings: &Settings) -> String {
    let mut copy = settings.clone();
    for source in &mut copy.sources {
        if !source.api_key.is_empty() {
            source.api_key = "(set)".into();
        }
    }
    toml::to_string_pretty(&copy).unwrap_or_default()
}

fn state_lines() -> Vec<String> {
    let state = EngineState::load();
    toml::to_string_pretty(&state).unwrap_or_default().lines().filter(|line| !line.trim().is_empty()).map(str::to_string).collect()
}
