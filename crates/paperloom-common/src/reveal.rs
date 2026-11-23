//! "Show in folder": opens the system file manager with a file selected, so a
//! person can send that file to someone (a wallpaper imported from their own
//! files can't be shared as a code).

use std::path::Path;

/// Shows `path` in the file manager, selected where the system allows it
/// (Explorer, Finder, and file managers on Linux that implement the
/// `FileManager1` service); otherwise opens the folder that holds it.
/// `false` when nothing could be opened.
pub fn show_in_folder(path: &Path) -> bool {
    if !path.exists() {
        return path.parent().is_some_and(open_folder);
    }
    select(path) || path.parent().is_some_and(open_folder)
}

#[cfg(windows)]
fn select(path: &Path) -> bool {
    use std::os::windows::process::CommandExt;
    // Explorer's exit code is 1 even when it worked, so only a failed start counts.
    crate::process::command("explorer.exe").raw_arg(format!("/select,\"{}\"", path.display())).spawn().is_ok()
}

#[cfg(windows)]
fn open_folder(dir: &Path) -> bool {
    crate::process::command("explorer.exe").arg(dir).spawn().is_ok()
}

#[cfg(target_os = "macos")]
fn select(path: &Path) -> bool {
    std::process::Command::new("open").arg("-R").arg(path).status().is_ok_and(|status| status.success())
}

#[cfg(target_os = "macos")]
fn open_folder(dir: &Path) -> bool {
    std::process::Command::new("open").arg(dir).status().is_ok_and(|status| status.success())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn select(path: &Path) -> bool {
    let uri = file_uri(path);
    std::process::Command::new("gdbus")
        .args(["call", "--session", "--dest", "org.freedesktop.FileManager1", "--object-path", "/org/freedesktop/FileManager1", "--method", "org.freedesktop.FileManager1.ShowItems"])
        .arg(format!("['{uri}']"))
        .arg("")
        .output()
        .is_ok_and(|out| out.status.success())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_folder(dir: &Path) -> bool {
    std::process::Command::new("xdg-open").arg(dir).spawn().is_ok()
}

/// `file:///…` with everything outside the safe set percent-encoded.
#[cfg(any(all(unix, not(target_os = "macos")), test))]
fn file_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => uri.push(byte as char),
            other => uri.push_str(&format!("%{other:02X}")),
        }
    }
    uri
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn the_uri_encodes_spaces_and_quotes() {
        assert_eq!(file_uri(Path::new("/media/My SSD/it's here.mp4")), "file:///media/My%20SSD/it%27s%20here.mp4");
    }
}
