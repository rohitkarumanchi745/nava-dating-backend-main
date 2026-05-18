//! HLS transcoding — converts an uploaded video into 3-variant adaptive-bitrate
//! HLS using FFmpeg. Requires `ffmpeg` / `ffprobe` on PATH.
//!
//! Pipeline (optimized — single-pass where possible):
//!   1. probe_video()        — ffprobe duration + codec; skip re-encode if not needed
//!   2. normalize_and_hls()  — single FFmpeg pass: trim + scale + HLS output (no double-encode)
//!   Fallback: normalize_video() + transcode_to_hls() when single-pass isn't viable
//!
//! Output layout under `{upload_dir}/{subdir}/hls/{id}/`:
//!   master.m3u8      ← master playlist (returned as the stored hls_url)
//!   360p/playlist.m3u8 + seg*.ts
//!   720p/playlist.m3u8 + seg*.ts
//!   1080p/playlist.m3u8 + seg*.ts
//!
//! `subdir` is "reels" for reels, "spots" for spots — both share the ladder
//! and pipeline, only the on-disk + URL prefix differs.
//!
//! AVPlayer on iOS speaks HLS natively — no client code change needed.

use tokio::fs;
use tokio::process::Command;

/// Max file size (50MB). Videos under this skip resolution reduction.
const MAX_NORMALIZED_SIZE: u64 = 50 * 1024 * 1024;

/// Video metadata from ffprobe
pub struct ProbeResult {
    pub duration_secs: f64,
    pub codec: String,
    pub width: u32,
}

/// Fast ffprobe to get duration, codec, and width — avoids unnecessary re-encoding.
pub async fn probe_video(input_path: &str) -> Result<ProbeResult, String> {
    let output = Command::new("ffprobe")
        .args([
            "-v", "quiet",
            "-print_format", "json",
            "-show_format", "-show_streams",
            "-select_streams", "v:0",
            input_path,
        ])
        .output()
        .await
        .map_err(|e| format!("ffprobe launch failed: {e}"))?;

    if !output.status.success() {
        return Err("ffprobe failed".to_string());
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("ffprobe parse failed: {e}"))?;

    let duration_secs = json["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(999.0);

    let stream = &json["streams"][0];
    let codec = stream["codec_name"].as_str().unwrap_or("unknown").to_string();
    let width = stream["width"].as_u64().unwrap_or(0) as u32;

    Ok(ProbeResult { duration_secs, codec, width })
}

/// Single-pass: normalize + HLS in one FFmpeg command.
/// Trims to 30s, scales to 3 variants, outputs HLS directly.
/// ~3x faster than the old normalize-then-transcode pipeline.
pub async fn normalize_and_hls(
    id: i64,
    input_path: &str,
    upload_dir: &str,
    subdir: &str,
) -> Result<String, String> {
    let out = format!("{}/{}/hls/{}", upload_dir, subdir, id);

    for sub in &["360p", "720p", "1080p"] {
        fs::create_dir_all(format!("{}/{}", out, sub))
            .await
            .map_err(|e| format!("mkdir {sub} failed: {e}"))?;
    }

    let filter = "[0:v]split=3[v1][v2][v3];\
         [v1]scale=min(360\\,iw):-2[360p];\
         [v2]scale=min(720\\,iw):-2[720p];\
         [v3]scale=min(1080\\,iw):-2[1080p]"
        .to_string();

    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-i", input_path,
            "-t", "30",
            "-filter_complex", &filter,

            // ── 360p ──
            "-map", "[360p]", "-map", "0:a?",
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "23", "-b:v", "800k",
            "-pix_fmt", "yuv420p",
            "-c:a", "aac", "-b:a", "96k",
            "-hls_time", "2", "-hls_playlist_type", "vod",
            "-hls_segment_filename", &format!("{}/360p/seg%03d.ts", out),
            "-f", "hls", &format!("{}/360p/playlist.m3u8", out),

            // ── 720p ──
            "-map", "[720p]", "-map", "0:a?",
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "21", "-b:v", "2800k",
            "-pix_fmt", "yuv420p",
            "-c:a", "aac", "-b:a", "128k",
            "-hls_time", "2", "-hls_playlist_type", "vod",
            "-hls_segment_filename", &format!("{}/720p/seg%03d.ts", out),
            "-f", "hls", &format!("{}/720p/playlist.m3u8", out),

            // ── 1080p ──
            "-map", "[1080p]", "-map", "0:a?",
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "18", "-b:v", "5000k",
            "-pix_fmt", "yuv420p",
            "-c:a", "aac", "-b:a", "192k",
            "-hls_time", "2", "-hls_playlist_type", "vod",
            "-hls_segment_filename", &format!("{}/1080p/seg%03d.ts", out),
            "-f", "hls", &format!("{}/1080p/playlist.m3u8", out),
        ])
        .status()
        .await
        .map_err(|e| format!("FFmpeg launch failed: {e}"))?;

    if !status.success() {
        return Err(format!("FFmpeg exited with code {:?}", status.code()));
    }

    write_master_playlist(&out).await?;
    Ok(format!("/uploads/{}/hls/{}/master.m3u8", subdir, id))
}

/// Normalize an uploaded video (fallback path for oversized files):
/// - Trim to 30s, cap resolution, compress
/// - Uses veryfast preset (~3x faster than medium)
pub async fn normalize_video(input_path: &str) -> Result<String, String> {
    let base = input_path.rsplit_once('.').map(|(b, _)| b).unwrap_or(input_path);
    let normalized = format!("{}_normalized.mp4", base);

    let file_size = fs::metadata(input_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    if file_size <= MAX_NORMALIZED_SIZE {
        let status = Command::new("ffmpeg")
            .args([
                "-y", "-i", input_path,
                "-t", "30",
                "-c:v", "libx264", "-preset", "veryfast", "-crf", "20",
                "-pix_fmt", "yuv420p",
                "-c:a", "aac", "-b:a", "128k",
                "-movflags", "+faststart",
                &normalized,
            ])
            .status()
            .await
            .map_err(|e| format!("FFmpeg normalize launch failed: {e}"))?;

        if !status.success() {
            return Err(format!("FFmpeg normalize exited with code {:?}", status.code()));
        }

        replace_original(&normalized, input_path).await?;
        tracing::info!("Normalized video: {} (trimmed to 30s, kept original resolution)", input_path);
        return Ok(input_path.to_string());
    }

    // File is too large — progressively reduce resolution until it fits
    let caps = ["2160", "1440", "1080", "720", "480"];

    for cap in &caps {
        let vf = format!("scale=min({}\\,iw):-2", cap);

        let status = Command::new("ffmpeg")
            .args([
                "-y", "-i", input_path,
                "-t", "30",
                "-vf", &vf,
                "-c:v", "libx264", "-preset", "veryfast", "-crf", "20",
                "-pix_fmt", "yuv420p",
                "-c:a", "aac", "-b:a", "128k",
                "-movflags", "+faststart",
                &normalized,
            ])
            .status()
            .await
            .map_err(|e| format!("FFmpeg normalize launch failed: {e}"))?;

        if !status.success() {
            return Err(format!("FFmpeg normalize exited with code {:?} at {}p", status.code(), cap));
        }

        let out_size = fs::metadata(&normalized).await.map(|m| m.len()).unwrap_or(0);

        if out_size <= MAX_NORMALIZED_SIZE {
            replace_original(&normalized, input_path).await?;
            tracing::info!(
                "Normalized video: {} (trimmed to 30s, capped at {}p, {:.1}MB)",
                input_path, cap, out_size as f64 / 1_048_576.0
            );
            return Ok(input_path.to_string());
        }

        tracing::info!("{}p still too large ({:.1}MB), trying lower", cap, out_size as f64 / 1_048_576.0);
    }

    replace_original(&normalized, input_path).await?;
    tracing::warn!("Video {} still over limit at 480p, using it anyway", input_path);
    Ok(input_path.to_string())
}

async fn replace_original(normalized: &str, original: &str) -> Result<(), String> {
    if let Err(e) = fs::rename(normalized, original).await {
        if let Err(e2) = fs::copy(normalized, original).await {
            return Err(format!("Failed to replace original: rename={e}, copy={e2}"));
        }
        let _ = fs::remove_file(normalized).await;
    }
    Ok(())
}

/// Transcode `input_path` to 3-variant HLS (standalone, used as fallback).
pub async fn transcode_to_hls(
    id: i64,
    input_path: &str,
    upload_dir: &str,
    subdir: &str,
) -> Result<String, String> {
    let out = format!("{}/{}/hls/{}", upload_dir, subdir, id);

    for sub in &["360p", "720p", "1080p"] {
        fs::create_dir_all(format!("{}/{}", out, sub))
            .await
            .map_err(|e| format!("mkdir {sub} failed: {e}"))?;
    }

    let filter = format!(
        "[0:v]split=3[v1][v2][v3];\
         [v1]scale=min(360\\,iw):-2[360p];\
         [v2]scale=min(720\\,iw):-2[720p];\
         [v3]scale=min(1080\\,iw):-2[1080p]"
    );

    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-i", input_path,
            "-filter_complex", &filter,

            "-map", "[360p]", "-map", "0:a?",
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "23", "-b:v", "800k",
            "-c:a", "aac", "-b:a", "96k",
            "-hls_time", "2", "-hls_playlist_type", "vod",
            "-hls_segment_filename", &format!("{}/360p/seg%03d.ts", out),
            "-f", "hls", &format!("{}/360p/playlist.m3u8", out),

            "-map", "[720p]", "-map", "0:a?",
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "21", "-b:v", "2800k",
            "-c:a", "aac", "-b:a", "128k",
            "-hls_time", "2", "-hls_playlist_type", "vod",
            "-hls_segment_filename", &format!("{}/720p/seg%03d.ts", out),
            "-f", "hls", &format!("{}/720p/playlist.m3u8", out),

            "-map", "[1080p]", "-map", "0:a?",
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "18", "-b:v", "5000k",
            "-c:a", "aac", "-b:a", "192k",
            "-hls_time", "2", "-hls_playlist_type", "vod",
            "-hls_segment_filename", &format!("{}/1080p/seg%03d.ts", out),
            "-f", "hls", &format!("{}/1080p/playlist.m3u8", out),
        ])
        .status()
        .await
        .map_err(|e| format!("FFmpeg launch failed: {e}"))?;

    if !status.success() {
        return Err(format!("FFmpeg exited with code {:?}", status.code()));
    }

    write_master_playlist(&out).await?;
    Ok(format!("/uploads/{}/hls/{}/master.m3u8", subdir, id))
}

async fn write_master_playlist(out_dir: &str) -> Result<(), String> {
    let master = "#EXTM3U\n\
#EXT-X-VERSION:3\n\
\n\
#EXT-X-STREAM-INF:BANDWIDTH=800000,CODECS=\"avc1.42c01e,mp4a.40.2\"\n\
360p/playlist.m3u8\n\
\n\
#EXT-X-STREAM-INF:BANDWIDTH=2800000,CODECS=\"avc1.42c01e,mp4a.40.2\"\n\
720p/playlist.m3u8\n\
\n\
#EXT-X-STREAM-INF:BANDWIDTH=5000000,CODECS=\"avc1.42c01e,mp4a.40.2\"\n\
1080p/playlist.m3u8\n";

    fs::write(format!("{}/master.m3u8", out_dir), master)
        .await
        .map_err(|e| format!("write master.m3u8 failed: {e}"))?;

    Ok(())
}
