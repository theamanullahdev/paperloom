//! Linux (polkit's `pkexec`) and macOS (the administrator prompt).

use super::{interpret_exit, Failure};

#[cfg(target_os = "macos")]
pub fn verify(reason: &str) -> Result<(), Failure> {
    // Quotes and backslashes in `reason` would end the AppleScript string.
    let reason: String = reason.chars().filter(|c| c.is_alphanumeric() || " .,'-".contains(*c)).collect();
    let script = format!("do shell script \"true\" with prompt \"{reason}\" with administrator privileges");
    let output = crate::process::command("osascript").args(["-e", &script]).output();
    match output {
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            interpret_exit("osascript", out.status.code(), &stderr, false)
        }
        Err(err) => interpret_exit("osascript", None, &err.to_string(), err.kind() == std::io::ErrorKind::NotFound),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn verify(_reason: &str) -> Result<(), Failure> {
    // Inside a Flatpak sandbox the host's polkit isn't reachable.
    if std::env::var_os("FLATPAK_ID").is_some() {
        return Err(Failure::Unavailable("This copy of Paperloom runs in a sandbox and can't show the system login window. Use the manual way below.".into()));
    }
    match crate::process::command("pkexec").arg("true").output() {
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            interpret_exit("pkexec", out.status.code(), &stderr, false)
        }
        Err(err) => interpret_exit("pkexec", None, &err.to_string(), err.kind() == std::io::ErrorKind::NotFound),
    }
}
