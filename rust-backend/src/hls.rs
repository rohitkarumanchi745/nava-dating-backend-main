//! HLS transcoding — converts an uploaded video into 3-variant adaptive-bitrate
//! HLS using FFmpeg. Requires `ffmpeg` on PATH.
//!
//! Pipeline:
//!   1. normalize_video()  — trim to 30s, cap at 1080p, compress to high-quality H.264
//!   2. transcode_to_hls() — split normalized file into 360p/720p/1080p HLS variants
//!
//! Output layout under `{upload_dir}/reels/hls/{reel_id}/`:
//!   master.m3u8      ← master playlist (returned as the stored hls_url)
//!   360p/playlist.m3u8 + seg*.ts
//!   720p/playlist.m3u8 + seg*.ts
//!   1080p/playlist.m3u8 + seg*.ts
//!
//! AVPlayer on iOS speaks HLS natively — no client code change needed.

use tokio::fs;
use tokio::process::Command;

/// Max file size (50MB). Videos under this skip resolution reduction.
const MAX_NORMALIZED_SIZE: u64 = 50 * 1024 * 1024;

/// Normalize an uploaded video:
/// - Always trim to 30s max
/// - Only reduce resolution if the file exceeds MAX_NORMALIZED_SIZE
/// - If a 4K video fits within the size limit, it stays 4K
pub async fn normalize_video(input_path: &str) -> Result<String, String> {
    let base = input_path.rsplit_once('.').map(|(b, _)| b).unwrap_or(input_path);
    let normalized = format!("{}_normalized.mp4", base);

    let file_size = fs::metadata(input_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    // Step 1: Always trim to 30s. If file is within size limit, keep original resolution.
    if file_size <= MAX_NORMALIZED_SIZE {
        let status = Command::new("ffmpeg")
            .args([
                "-y", "-i", input_path,
                "-t", "30",
                "-c:v", "libx264", "-preset", "medium", "-crf", "20",
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

    // Step 2: File is too large — progressively reduce resolution until it fits.
    // Try each resolution cap from highest to lowest.
    let caps = ["2160", "1440", "1080", "720", "480"];

    for cap in &caps {
        let vf = format!("scale=min({}\\,iw):-2", cap);

        let status = Command::new("ffmpeg")
            .args([
                "-y", "-i", input_path,
                "-t", "30",
                "-vf", &vf,
                "-c:v", "libx264", "-preset", "medium", "-crf", "20",
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

    // If even 480p is too large, use the last result anyway
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

/// Transcode `input_path` to 3-variant HLS.
///
/// Returns the **relative URL** to the master playlist,
/// e.g. `/uploads/reels/hls/42/master.m3u8`.
pub async fn transcode_to_hls(
    reel_id: i64,
    input_path: &str,
    upload_dir: &str,
) -> Result<String, String> {
    let out = format!("{}/reels/hls/{}", upload_dir, reel_id);

    for sub in &["360p", "720p", "1080p"] {
        fs::create_dir_all(format!("{}/{}", out, sub))
            .await
            .map_err(|e| format!("mkdir {sub} failed: {e}"))?;
    }

    // scale=min(TARGET,iw):-2
    //   • Never upscales (capped at source width)
    //   • -2 auto-computes height divisible by 2
    //   • Works for portrait (9:16) and landscape — scales by width
    //   • \, is FFmpeg's escaped comma inside a filter-option value
    let filter = format!(
        "[0:v]split=3[v1][v2][v3];\
         [v1]scale=min(360\\,iw):-2[360p];\
         [v2]scale=min(720\\,iw):-2[720p];\
         [v3]scale=min(1080\\,iw):-2[1080p]"
    );

    // Build args list — each flag/value is a separate element (no shell quoting needed)
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-i", input_path,
            "-filter_complex", &filter,

            // ── 360p variant ──────────────────────────────────────────────
            "-map", "[360p]", "-map", "0:a?",
            "-c:v", "libx264", "-preset", "fast", "-crf", "23", "-b:v", "800k",
            "-c:a", "aac", "-b:a", "96k",
            "-hls_time", "2", "-hls_playlist_type", "vod",
            "-hls_segment_filename", &format!("{}/360p/seg%03d.ts", out),
            "-f", "hls", &format!("{}/360p/playlist.m3u8", out),

            // ── 720p variant ──────────────────────────────────────────────
            "-map", "[720p]", "-map", "0:a?",
            "-c:v", "libx264", "-preset", "fast", "-crf", "21", "-b:v", "2800k",
            "-c:a", "aac", "-b:a", "128k",
            "-hls_time", "2", "-hls_playlist_type", "vod",
            "-hls_segment_filename", &format!("{}/720p/seg%03d.ts", out),
            "-f", "hls", &format!("{}/720p/playlist.m3u8", out),

            // ── 1080p variant ─────────────────────────────────────────────
            "-map", "[1080p]", "-map", "0:a?",
            "-c:v", "libx264", "-preset", "fast", "-crf", "18", "-b:v", "5000k",
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

    // Write master playlist — AVPlayer picks the right variant automatically
    // based on available bandwidth (BANDWIDTH hint) and display size.
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

    fs::write(format!("{}/master.m3u8", out), master)
        .await
        .map_err(|e| format!("write master.m3u8 failed: {e}"))?;

    Ok(format!("/uploads/reels/hls/{}/master.m3u8", reel_id))
}
