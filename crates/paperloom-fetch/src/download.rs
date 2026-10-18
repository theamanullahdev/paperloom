//! One resumable download attempt. A failed attempt leaves its `.part`
//! file behind, and the next attempt continues from that byte offset with
//! an HTTP `Range` request — so a dropped connection costs only the time
//! it was down, never the bytes already fetched.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::http::{self, classify_status, classify_transport, FetchError, Lane};

/// Runs one attempt: continues `part` if it exists, and on success renames
/// it to `dest`. `progress(done, total)` is called as bytes arrive.
pub fn download(
    url: &str,
    part: &Path,
    dest: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<(), FetchError> {
    let io = |err: std::io::Error| FetchError::Transient(format!("disk error: {err}"));

    if let Some(parent) = part.parent() {
        fs::create_dir_all(parent).map_err(io)?;
    }
    let existing = part.metadata().map(|m| m.len()).unwrap_or(0);

    // Page sources may hand over the wallpaper's page instead of its video.
    let url = crate::generic::resolve_video(url)?;
    http::wait_turn(Lane::Download);
    let mut request = http::agent().get(&url);
    if existing > 0 {
        request = request.header("Range", format!("bytes={existing}-"));
    }
    let mut response = request.call().map_err(classify_transport)?;

    let status = response.status().as_u16();
    let (mut done, total, append) = match status {
        206 => {
            let total = response
                .headers()
                .get("content-range")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.rsplit('/').next())
                .and_then(|v| v.parse::<u64>().ok());
            (existing, total, true)
        }
        // Server ignored (or we didn't send) Range: start over.
        200 => (0, content_length(&response), false),
        // Our offset is past the end — the part file is stale or already
        // complete-but-unrenamed. Drop it and let the retry start clean.
        416 => {
            let _ = fs::remove_file(part);
            return Err(FetchError::Transient("resume offset rejected, restarting".into()));
        }
        other => return Err(classify_status(other).unwrap_or_else(|| FetchError::Transient(format!("unexpected HTTP {other}")))),
    };

    let mut file = OpenOptions::new().create(true).write(true).append(append).truncate(!append).open(part).map_err(io)?;

    let mut reader = response.body_mut().as_reader();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut checked_header = done > 0;
    progress(done, total);

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(FetchError::Transient("cancelled".into()));
        }
        let read = reader.read(&mut buffer).map_err(|err| FetchError::Transient(err.to_string()))?;
        if read == 0 {
            break;
        }

        // A 200 with an HTML error/login page would otherwise be saved as
        // a "wallpaper". MP4 files carry `ftyp` at byte offset 4.
        if !checked_header {
            if read < 8 || &buffer[4..8] != b"ftyp" {
                drop(file);
                let _ = fs::remove_file(part);
                return Err(FetchError::Permanent("server did not return an MP4 video".into()));
            }
            checked_header = true;
        }

        file.write_all(&buffer[..read]).map_err(io)?;
        done += read as u64;
        progress(done, total);
    }
    file.flush().map_err(io)?;
    drop(file);

    if let Some(total) = total {
        if done != total {
            // Connection closed early; keep the part file so the next
            // attempt resumes from `done`.
            return Err(FetchError::Transient(format!("connection ended at {done} of {total} bytes")));
        }
    }

    fs::rename(part, dest).map_err(io)?;
    Ok(())
}

fn content_length(response: &ureq::http::Response<ureq::Body>) -> Option<u64> {
    response.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::path::PathBuf;

    fn fake_mp4(len: usize) -> Vec<u8> {
        let mut body: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        body[4..8].copy_from_slice(b"ftyp");
        body
    }

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("paperloom-dl-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Serves `handler(range_start)` once per connection: a closure that
    /// writes the whole raw response for a given requested start offset.
    fn serve(requests: usize, handler: impl Fn(u64, &mut std::net::TcpStream) + Send + 'static) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/video.mp4", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for _ in 0..requests {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut start = 0u64;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    if let Some(range) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        start = range.trim().trim_end_matches('-').parse().unwrap();
                    }
                }
                handler(start, &mut stream);
            }
        });
        url
    }

    #[test]
    fn resumes_from_part_file_after_dropped_connection() {
        let dir = scratch("resume");
        let full = fake_mp4(300_000);
        let served = full.clone();

        let url = serve(2, move |start, stream| {
            let total = served.len();
            if start == 0 {
                // Promise everything, deliver 40%, then hang up.
                let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nConnection: close\r\n\r\n");
                stream.write_all(head.as_bytes()).unwrap();
                stream.write_all(&served[..total * 4 / 10]).unwrap();
            } else {
                let rest = &served[start as usize..];
                let head = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{}/{total}\r\nConnection: close\r\n\r\n",
                    rest.len(),
                    total - 1
                );
                stream.write_all(head.as_bytes()).unwrap();
                stream.write_all(rest).unwrap();
            }
        });

        let (part, dest) = (dir.join("v.mp4.part"), dir.join("v.mp4"));
        let cancel = AtomicBool::new(false);

        let first = download(&url, &part, &dest, &cancel, |_, _| {});
        assert!(matches!(first, Err(FetchError::Transient(_))), "{first:?}");
        assert!(!dest.exists());
        assert_eq!(part.metadata().unwrap().len() as usize, full.len() * 4 / 10);

        download(&url, &part, &dest, &cancel, |_, _| {}).expect("resumed download");
        assert_eq!(fs::read(&dest).unwrap(), full, "resumed file must be byte-identical");
        assert!(!part.exists());
    }

    #[test]
    fn rejects_html_masquerading_as_video() {
        let dir = scratch("html");
        let url = serve(1, |_, stream| {
            let body = b"<html>blocked</html>";
            let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
        });

        let (part, dest) = (dir.join("v.mp4.part"), dir.join("v.mp4"));
        let result = download(&url, &part, &dest, &AtomicBool::new(false), |_, _| {});
        assert!(matches!(result, Err(FetchError::Permanent(_))), "{result:?}");
        assert!(!dest.exists() && !part.exists());
    }

    #[test]
    fn not_found_is_permanent_and_server_errors_are_transient() {
        let dir = scratch("status");
        for (code, expect_permanent) in [(404, true), (503, false), (429, false)] {
            let url = serve(1, move |_, stream| {
                stream.write_all(format!("HTTP/1.1 {code} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).unwrap();
            });
            let result = download(&url, &dir.join("a.part"), &dir.join("a.mp4"), &AtomicBool::new(false), |_, _| {});
            assert_eq!(matches!(result, Err(FetchError::Permanent(_))), expect_permanent, "HTTP {code}: {result:?}");
        }
    }
}
