//! Looking at a video file and deciding whether it is worth re-encoding for
//! use as a wallpaper.
//!
//! Probing uses the `ffmpeg` that ships with Paperloom (`ffmpeg -i` prints
//! the streams to stderr), so no extra tool is needed. A wallpaper plays
//! all day: one the graphics card can decode, at the size and frame rate the
//! screen actually shows, costs a fraction of one it cannot. The policy
//! here is deliberately conservative — files that already suit are left
//! exactly as they are.

use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct MediaInfo {
    pub codec: String,
    pub pixel_format: String,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub has_audio: bool,
    pub duration_secs: f32,
}

/// Runs `ffmpeg -i` and parses what it prints. `None` when ffmpeg is
/// missing or the file isn't a readable video.
pub fn probe(path: &Path) -> Option<MediaInfo> {
    let output = crate::process::command("ffmpeg").args(["-hide_banner", "-i"]).arg(path).output().ok()?;
    parse_probe(&String::from_utf8_lossy(&output.stderr))
}

pub fn parse_probe(text: &str) -> Option<MediaInfo> {
    let video = text.lines().find(|line| line.contains("Video:") && line.contains("Stream #"))?;
    let after = video.split("Video:").nth(1)?;
    let mut parts = after.split(',').map(str::trim);
    let codec = parts.next()?.split_whitespace().next()?.to_lowercase();
    let pixel_format = parts.next().map(|part| part.split(['(', ' ']).next().unwrap_or_default().to_string()).unwrap_or_default();

    // "1920x1080" — the first token shaped like WxH after the codec field.
    let (width, height) = after.split(|c: char| c == ',' || c.is_whitespace()).find_map(|token| {
        let (w, h) = token.split_once('x')?;
        let (w, h): (u32, u32) = (w.parse().ok()?, h.parse().ok()?);
        (w >= 16 && h >= 16).then_some((w, h))
    })?;
    let fps = after.split(',').map(str::trim).find_map(|part| part.strip_suffix(" fps")?.trim().parse::<f32>().ok()).unwrap_or(0.0);

    let duration_secs = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("Duration:"))
        .and_then(|rest| rest.split(',').next())
        .and_then(|clock| {
            let mut fields = clock.trim().split(':');
            let (h, m, s): (f32, f32, f32) = (fields.next()?.parse().ok()?, fields.next()?.parse().ok()?, fields.next()?.parse().ok()?);
            Some(h * 3600.0 + m * 60.0 + s)
        })
        .unwrap_or(0.0);

    Some(MediaInfo { codec, pixel_format, width, height, fps, has_audio: text.lines().any(|line| line.contains("Audio:") && line.contains("Stream #")), duration_secs })
}

/// Why a file would be re-encoded.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// The size to scale down to (even numbers), if it is larger than the screen.
    pub scale_to: Option<(u32, u32)>,
    /// Cap the frame rate at this.
    pub fps_cap: Option<u32>,
    /// The codec or pixel format is one graphics cards often cannot decode.
    pub change_format: bool,
}

pub const FPS_LIMIT: u32 = 30;

/// `None`: leave the file alone. `screen`: the primary display's size.
pub fn plan(info: &MediaInfo, screen: Option<(u32, u32)>) -> Option<Plan> {
    // Still pictures cost nothing to show; leave them as they are.
    if matches!(info.codec.as_str(), "mjpeg" | "png" | "bmp" | "webp" | "tiff" | "jpeg2000" | "ppm") || info.duration_secs < 0.2 && info.fps < 1.5 && info.codec != "gif" {
        return None;
    }
    let friendly = matches!(
        (info.codec.as_str(), info.pixel_format.as_str()),
        ("h264", "yuv420p" | "yuvj420p") | ("hevc", "yuv420p" | "yuv420p10le") | ("vp9", "yuv420p")
    );

    // Bigger than the screen (with a little room): the extra pixels are
    // decoded and thrown away every frame.
    let scale_to = screen.and_then(|(sw, sh)| {
        let (limit_w, limit_h) = (sw.max(1280), sh.max(720));
        if info.width as f32 > limit_w as f32 * 1.15 || info.height as f32 > limit_h as f32 * 1.15 {
            let ratio = (limit_w as f32 / info.width as f32).min(limit_h as f32 / info.height as f32);
            let even = |value: f32| ((value / 2.0).round() as u32 * 2).max(2);
            Some((even(info.width as f32 * ratio), even(info.height as f32 * ratio)))
        } else {
            None
        }
    });
    let fps_cap = (info.fps > FPS_LIMIT as f32 + 1.0).then_some(FPS_LIMIT);

    (!friendly || scale_to.is_some() || fps_cap.is_some()).then_some(Plan { scale_to, fps_cap, change_format: !friendly })
}

/// The ffmpeg arguments (after `-i <input>`) that produce a wallpaper-ready
/// H.264 8-bit file at `output`. Progress goes to stdout as `key=value`.
pub fn encode_args(plan: &Plan, info: &MediaInfo, input: &Path, output: &Path) -> Vec<std::ffi::OsString> {
    let mut filters = Vec::new();
    if let Some((w, h)) = plan.scale_to {
        filters.push(format!("scale={w}:{h}:flags=bicubic"));
    }
    if plan.fps_cap.is_some() {
        filters.push(format!("fps={FPS_LIMIT}"));
    }
    filters.push("format=yuv420p".to_string());
    let fps = plan.fps_cap.map_or(info.fps.round().max(1.0) as u32, |cap| cap.min(info.fps.round().max(1.0) as u32));

    let mut args: Vec<std::ffi::OsString> = ["-hide_banner", "-nostdin", "-y", "-i"].iter().map(Into::into).collect();
    args.push(input.into());
    for item in [
        "-map", "0:v:0", "-an", "-sn", "-vf", &filters.join(","), "-c:v", "libx264", "-preset", "veryfast", "-crf", "21", "-profile:v", "high", "-g", &(fps * 2).to_string(), "-movflags", "+faststart",
        "-progress", "pipe:1", "-nostats",
    ] {
        args.push(item.into());
    }
    args.push(output.into());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'a.mp4':\n  Duration: 00:00:12.50, start: 0.000000, bitrate: 9000 kb/s\n  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, bt709, progressive), 3840x2160 [SAR 1:1 DAR 16:9], 8800 kb/s, 60 fps, 60 tbr, 15360 tbn (default)\n  Stream #0:1[0x2](und): Audio: aac (LC), 48000 Hz, stereo, fltp, 128 kb/s (default)\n";

    #[test]
    fn parses_ffmpeg_output() {
        let info = parse_probe(SAMPLE).unwrap();
        assert_eq!((info.codec.as_str(), info.pixel_format.as_str()), ("h264", "yuv420p"));
        assert_eq!((info.width, info.height, info.fps as u32), (3840, 2160, 60));
        assert!(info.has_audio);
        assert!((info.duration_secs - 12.5).abs() < 0.01);
    }

    #[test]
    fn plans_only_when_worthwhile() {
        let info = parse_probe(SAMPLE).unwrap();
        // 4K60 on a 1080p screen: scale down and cap fps.
        let plan = plan(&info, Some((1920, 1080))).unwrap();
        assert_eq!(plan.scale_to, Some((1920, 1080)));
        assert_eq!(plan.fps_cap, Some(30));
        assert!(!plan.change_format);

        // 1080p30 H.264 on a 1080p screen: leave it alone.
        let fine = MediaInfo { width: 1920, height: 1080, fps: 30.0, ..info.clone() };
        assert_eq!(super::plan(&fine, Some((1920, 1080))), None);

        // 10-bit H.264 cannot be hardware-decoded by many cards.
        let ten_bit = MediaInfo { pixel_format: "yuv420p10le".into(), ..fine.clone() };
        assert!(super::plan(&ten_bit, Some((1920, 1080))).unwrap().change_format);

        // AV1 is converted.
        let av1 = MediaInfo { codec: "av1".into(), ..fine };
        assert!(super::plan(&av1, Some((1920, 1080))).unwrap().change_format);
    }
}
