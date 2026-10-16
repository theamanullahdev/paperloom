//! Disk cache for online preview images, so scrolling back through results
//! (or reopening the app) never re-downloads them.

use std::path::PathBuf;

use paperloom_common::online_cache_dir;

use crate::http::{self, FetchError};

const MAX_IMAGE_BYTES: u64 = 4 * 1024 * 1024;

/// Returns the cached file for `url`, fetching it first if needed.
pub fn fetch(url: &str) -> Result<PathBuf, FetchError> {
    let dir = online_cache_dir();
    std::fs::create_dir_all(&dir).map_err(|err| FetchError::Transient(format!("cache dir: {err}")))?;

    let extension = if url.ends_with(".png") { "png" } else { "jpg" };
    let path = dir.join(format!("{:016x}.{extension}", fnv1a(url)));
    if path.metadata().is_ok_and(|meta| meta.len() > 0) {
        return Ok(path);
    }

    let bytes = http::get_bytes(url, MAX_IMAGE_BYTES)?;
    // Write-then-rename so a half-written file is never mistaken for a
    // cached image after a crash.
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, bytes).map_err(|err| FetchError::Transient(format!("cache write: {err}")))?;
    std::fs::rename(&temp, &path).map_err(|err| FetchError::Transient(format!("cache write: {err}")))?;
    Ok(path)
}

fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3))
}
