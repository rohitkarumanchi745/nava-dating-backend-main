//! Async photo processing pipeline with per-stage timeouts.
//!
//! Stages run sequentially after upload:
//! 1. EXIF strip + resize to canonical max (1080px)
//! 2. Vision analysis (quality, NSFW, liveness, face embedding — single pass)
//! 3. Gate checks on vision results (quality, NSFW, liveness thresholds)
//! 4. Duplicate-face check (per user + global blocklist)
//!
//! Each stage has a configurable timeout. If a stage times out,
//! the photo is flagged `needs_review` rather than rejected.

use std::sync::Arc;
use std::time::Duration;

use image::DynamicImage;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::Mutex;
use tokio::time::timeout;
use tracing::{info, warn};

use crate::error::AppError;
use crate::services::media_optimizer::MediaOptimizer;
use crate::services::moderation::ModerationPipeline;
use crate::vision::quality::PhotoQualityResult;
use crate::vision::{VisionAnalysis, VisionAnalyzer};

/// Overall pipeline verdict for a photo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PhotoVerdict {
    /// Photo passed all checks — safe to serve immediately.
    Approved,
    /// At least one stage timed out or returned a borderline score — queue for human review.
    NeedsReview,
    /// Photo clearly violates policy (NSFW, duplicate banned face, etc.).
    Rejected,
}

/// Per-stage result attached to a processed photo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageResult {
    pub stage: String,
    pub passed: bool,
    pub score: Option<f64>,
    pub detail: Option<String>,
    pub timed_out: bool,
}

/// Complete pipeline output for a single photo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhotoPipelineResult {
    pub verdict: PhotoVerdict,
    pub stages: Vec<StageResult>,
    pub quality: Option<PhotoQualityResult>,
    /// ArcFace 128-dim embedding (for duplicate-face indexing).
    #[serde(skip)]
    pub face_embedding: Option<Vec<f32>>,
    /// Resized image ready for rendition generation.
    #[serde(skip)]
    pub processed_image: Option<DynamicImage>,
}

/// Per-stage timeout configuration (milliseconds).
#[derive(Debug, Clone)]
pub struct PipelineTimeouts {
    /// EXIF strip + resize
    pub resize_ms: u64,
    /// Full vision analysis (quality + NSFW + liveness + arcface)
    pub vision_ms: u64,
    /// Duplicate-face DB lookup
    pub duplicate_ms: u64,
}

impl Default for PipelineTimeouts {
    fn default() -> Self {
        Self {
            resize_ms: 500,
            vision_ms: 2000, // Vision runs all models in one pass
            duplicate_ms: 300,
        }
    }
}

/// Maximum dimension for the canonical resized image.
const CANONICAL_MAX_DIM: u32 = 1080;

/// Run the full photo pipeline on raw image bytes.
///
/// Returns a `PhotoPipelineResult` with per-stage scores and an overall verdict.
/// If the vision analyzer is unavailable, vision stages are skipped and the
/// photo is marked `NeedsReview`.
pub async fn run_pipeline(
    image_bytes: Vec<u8>,
    user_id: i32,
    photo_slot: &str,
    vision: Option<Arc<Mutex<VisionAnalyzer>>>,
    moderation: Option<Arc<ModerationPipeline>>,
    pool: &PgPool,
    timeouts: &PipelineTimeouts,
) -> Result<PhotoPipelineResult, AppError> {
    let mut stages: Vec<StageResult> = Vec::new();
    let mut verdict = PhotoVerdict::Approved;
    let mut quality_result: Option<PhotoQualityResult> = None;
    let mut face_embedding: Option<Vec<f32>> = None;

    // ── Stage 1: EXIF strip + resize ────────────────────────────────────
    let bytes_clone = image_bytes.clone();
    let resized = run_with_timeout(
        "resize",
        timeouts.resize_ms,
        &mut stages,
        &mut verdict,
        tokio::task::spawn_blocking(move || {
            let img = image::load_from_memory(&bytes_clone)
                .map_err(|_| AppError::bad_request("Invalid image"))?;
            // Strip EXIF by re-encoding through DynamicImage (pixel data only).
            // Resize if larger than canonical max.
            let resized = if img.width() > CANONICAL_MAX_DIM || img.height() > CANONICAL_MAX_DIM {
                img.resize(
                    CANONICAL_MAX_DIM,
                    CANONICAL_MAX_DIM,
                    image::imageops::FilterType::Lanczos3,
                )
            } else {
                img
            };
            Ok::<DynamicImage, AppError>(resized)
        }),
    )
    .await;

    let image = match resized {
        Some(Ok(Ok(img))) => img,
        Some(Ok(Err(e))) => return Err(e),
        Some(Err(_join_err)) => return Err(AppError::internal("Image decode task panicked")),
        None => {
            // Timeout on decode/resize — still try to load without resize
            image::load_from_memory(&image_bytes)
                .map_err(|_| AppError::bad_request("Invalid image"))?
        }
    };

    // ── Stage 2: Vision analysis (single pass: quality + NSFW + liveness + arcface) ──
    let mut analysis: Option<VisionAnalysis> = None;

    if let Some(ref vis) = vision {
        let vis_clone = vis.clone();
        let img_clone = image.clone();
        let vision_result = run_with_timeout(
            "vision",
            timeouts.vision_ms,
            &mut stages,
            &mut verdict,
            tokio::task::spawn_blocking(move || {
                let vis_guard = vis_clone.blocking_lock();
                vis_guard
                    .analyze_image(&img_clone)
                    .map_err(|e| AppError::internal(e.to_string()))
            }),
        )
        .await;

        match vision_result {
            Some(Ok(Ok(va))) => { analysis = Some(va); }
            Some(Ok(Err(_))) | Some(Err(_)) => {
                // Vision failed or panicked — treat as needs_review
                if verdict == PhotoVerdict::Approved {
                    verdict = PhotoVerdict::NeedsReview;
                }
            }
            None => {} // Timeout already handled by run_with_timeout
        }
        // If timed out, verdict is already NeedsReview via run_with_timeout
    } else {
        stages.push(StageResult {
            stage: "vision".to_string(),
            passed: true,
            score: None,
            detail: Some("Vision analyzer unavailable — skipped".to_string()),
            timed_out: false,
        });
        if verdict == PhotoVerdict::Approved {
            verdict = PhotoVerdict::NeedsReview;
        }
    }

    // ── Stage 3: Gate checks on vision results ──────────────────────────
    if let Some(ref va) = analysis {
        // Build quality result from vision analysis
        let qr = PhotoQualityResult {
            composite_score: va.quality_score,
            blur_score: va.blur_score,
            low_light_score: va.low_light_score,
            face_ratio: va.face_ratio,
            flags: va.quality_flags.clone(),
        };

        // Quality gate
        if qr.composite_score < 0.25 {
            stages.push(StageResult {
                stage: "quality_gate".to_string(),
                passed: false,
                score: Some(qr.composite_score as f64),
                detail: Some(format!("Quality too low: flags={:?}", qr.flags)),
                timed_out: false,
            });
            verdict = PhotoVerdict::Rejected;
        } else if qr.composite_score < 0.45 {
            stages.push(StageResult {
                stage: "quality_gate".to_string(),
                passed: false,
                score: Some(qr.composite_score as f64),
                detail: Some("Borderline quality — queued for review".to_string()),
                timed_out: false,
            });
            if verdict == PhotoVerdict::Approved {
                verdict = PhotoVerdict::NeedsReview;
            }
        } else {
            stages.push(StageResult {
                stage: "quality_gate".to_string(),
                passed: true,
                score: Some(qr.composite_score as f64),
                detail: None,
                timed_out: false,
            });
        }
        quality_result = Some(qr);

        // NSFW gate
        if va.inappropriate_content {
            stages.push(StageResult {
                stage: "nsfw_gate".to_string(),
                passed: false,
                score: va.confidence_scores.get("nsfw").map(|s| *s as f64),
                detail: Some("NSFW content detected".to_string()),
                timed_out: false,
            });
            verdict = PhotoVerdict::Rejected;
        } else {
            let nsfw_score = va.confidence_scores.get("nsfw").copied().unwrap_or(0.0);
            if nsfw_score >= 0.4 {
                stages.push(StageResult {
                    stage: "nsfw_gate".to_string(),
                    passed: false,
                    score: Some(nsfw_score as f64),
                    detail: Some("Borderline NSFW — queued for review".to_string()),
                    timed_out: false,
                });
                if verdict == PhotoVerdict::Approved {
                    verdict = PhotoVerdict::NeedsReview;
                }
            } else {
                stages.push(StageResult {
                    stage: "nsfw_gate".to_string(),
                    passed: true,
                    score: Some(nsfw_score as f64),
                    detail: None,
                    timed_out: false,
                });
            }
        }

        // Liveness gate
        if va.authenticity_score < 0.2 {
            stages.push(StageResult {
                stage: "liveness_gate".to_string(),
                passed: false,
                score: Some(va.authenticity_score as f64),
                detail: Some("No real face detected".to_string()),
                timed_out: false,
            });
            if verdict == PhotoVerdict::Approved {
                verdict = PhotoVerdict::NeedsReview;
            }
        } else {
            stages.push(StageResult {
                stage: "liveness_gate".to_string(),
                passed: true,
                score: Some(va.authenticity_score as f64),
                detail: None,
                timed_out: false,
            });
        }

        // Store face embedding for duplicate check
        if !va.style_embedding.is_empty() {
            face_embedding = Some(va.style_embedding.clone());
        }
    }

    // Early exit if rejected
    if verdict == PhotoVerdict::Rejected {
        return Ok(PhotoPipelineResult {
            verdict,
            stages,
            quality: quality_result,
            face_embedding: None,
            processed_image: Some(image),
        });
    }

    // ── Stage 4: Duplicate-face check ───────────────────────────────────
    if let (Some(ref emb), Some(ref mod_pipeline)) = (&face_embedding, &moderation) {
        let emb_clone = emb.clone();
        let mod_clone = mod_pipeline.clone();
        let pool_clone = pool.clone();
        let dup = run_with_timeout(
            "duplicate_face",
            timeouts.duplicate_ms,
            &mut stages,
            &mut verdict,
            async move {
                let result = mod_clone
                    .check_duplicate_face(&pool_clone, &emb_clone, user_id)
                    .await;
                Ok::<_, AppError>(result)
            },
        )
        .await;

        if let Some(Ok(dup_result)) = dup {
            if dup_result.is_duplicate {
                stages.push(StageResult {
                    stage: "duplicate_gate".to_string(),
                    passed: false,
                    score: Some(dup_result.similarity),
                    detail: Some(format!(
                        "Duplicate face detected (matching user: {:?})",
                        dup_result.matching_user_id
                    )),
                    timed_out: false,
                });
                verdict = PhotoVerdict::Rejected;
            }
        }
    }

    // ── Log quality results to photo_quality_log ────────────────────────
    if let Some(ref qr) = quality_result {
        let _ = sqlx::query(
            r#"INSERT INTO photo_quality_log
               (user_id, photo_slot, composite_score, blur_score, low_light_score, face_ratio, flags)
               VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
        )
        .bind(user_id as i64)
        .bind(photo_slot)
        .bind(qr.composite_score as f64)
        .bind(qr.blur_score as f64)
        .bind(qr.low_light_score as f64)
        .bind(qr.face_ratio as f64)
        .bind(serde_json::to_value(&qr.flags).unwrap_or_default())
        .execute(pool)
        .await;
    }

    info!(
        user_id = user_id,
        photo_slot = photo_slot,
        verdict = ?verdict,
        stages_count = stages.len(),
        "Photo pipeline complete"
    );

    Ok(PhotoPipelineResult {
        verdict,
        stages,
        quality: quality_result,
        face_embedding,
        processed_image: Some(image),
    })
}

/// Generate renditions for an approved/needs_review photo and return the output bytes.
pub fn generate_renditions(
    image: &DynamicImage,
    base_key: &str,
) -> Vec<crate::services::media_optimizer::RenditionOutput> {
    MediaOptimizer::generate_renditions(image, base_key)
}

/// Run a future with a timeout. If the timeout fires, push a `needs_review` stage result
/// and escalate the verdict to `NeedsReview` (never downgrades a `Rejected`).
async fn run_with_timeout<F, T>(
    stage_name: &str,
    timeout_ms: u64,
    stages: &mut Vec<StageResult>,
    verdict: &mut PhotoVerdict,
    future: F,
) -> Option<T>
where
    F: std::future::Future<Output = T>,
{
    match timeout(Duration::from_millis(timeout_ms), future).await {
        Ok(result) => {
            stages.push(StageResult {
                stage: stage_name.to_string(),
                passed: true,
                score: None,
                detail: None,
                timed_out: false,
            });
            Some(result)
        }
        Err(_elapsed) => {
            warn!(stage = stage_name, timeout_ms, "Pipeline stage timed out");
            stages.push(StageResult {
                stage: stage_name.to_string(),
                passed: false,
                score: None,
                detail: Some(format!("Timed out after {}ms — defaulting to needs_review", timeout_ms)),
                timed_out: true,
            });
            // Escalate to NeedsReview (but never downgrade Rejected)
            if *verdict == PhotoVerdict::Approved {
                *verdict = PhotoVerdict::NeedsReview;
            }
            None
        }
    }
}
