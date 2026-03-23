//! HLS transcoding — converts an uploaded video into 3-variant adaptive-bitrate
//! HLS using FFmpeg. Requires `ffmpeg` on PATH.
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
