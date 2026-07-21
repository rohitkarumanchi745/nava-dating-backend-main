//! Self-hosted ONNX face verification: UltraFace (detect) + ArcFace (embed).
//!
//! A *proper* face-identity pipeline — detect the largest face, crop it, and
//! compare ArcFace identity embeddings — replacing the old approach of
//! comparing whole-image "style embeddings" (scene similarity, not identity).
//!
//!   detect (UltraFace RFB-320, ~1.2MB) → crop+margin → resize 112×112
//!   → embed (ArcFace, 512-d) → L2-normalize → cosine similarity
//!
//! Enabled with FACE_VERIFY_ONNX=1. Model files are loaded from
//! VISION_MODEL_DIR and **auto-downloaded at boot when missing** (Railway's
//! filesystem is ephemeral, so this runs on every deploy — the detector is
//! tiny; the embedder default is ~249MB, override FACE_EMBEDDER_URL to use a
//! smaller ArcFace export). Inference is CPU via tract-onnx (already a dep).
//!
//! Known v1 limitation (documented, not hidden): no 5-point landmark
//! alignment — the crop is the padded detector box. That costs a few points
//! of accuracy vs aligned ArcFace / Rekognition; tune the decision threshold
//! with FACE_ONNX_MATCH_THRESHOLD (cosine, default 0.36).

use std::path::{Path, PathBuf};

use tract_onnx::prelude::tract_ndarray::Array4;
use tract_onnx::prelude::*;
use tracing::{info, warn};

type RunnableOnnx =
    SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>;

const DETECTOR_FILE: &str = "ultraface-rfb-320.onnx";
const EMBEDDER_FILE: &str = "arcface.onnx";
const DEFAULT_DETECTOR_URL: &str = "https://github.com/onnx/models/raw/main/validated/vision/body_analysis/ultraface/models/version-RFB-320.onnx";
const DEFAULT_EMBEDDER_URL: &str = "https://github.com/onnx/models/raw/main/validated/vision/body_analysis/arcface/model/arcfaceresnet100-8.onnx";

// UltraFace RFB-320 input geometry
const DET_W: usize = 320;
const DET_H: usize = 240;
/// Minimum detector confidence to accept a face box.
const DET_MIN_SCORE: f32 = 0.7;
/// Margin added around the detector box before cropping (fraction of box size).
const CROP_MARGIN: f32 = 0.25;
// ArcFace input geometry
const EMB_SIZE: usize = 112;

pub struct FaceVerifier {
    detector: RunnableOnnx,
    embedder: RunnableOnnx,
    /// Cosine-similarity decision threshold (same person ⇔ cosine ≥ threshold).
    pub match_threshold: f32,
}

impl FaceVerifier {
    /// Load (downloading missing model files first) when FACE_VERIFY_ONNX is
    /// enabled. Returns None when disabled or when setup fails — callers fall
    /// back to other verification backends.
    pub async fn from_env() -> Option<Self> {
        use std::env;
        let enabled = env::var("FACE_VERIFY_ONNX")
            .map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(false);
        if !enabled {
            return None;
        }
        let dir = env::var("VISION_MODEL_DIR").unwrap_or_else(|_| "models".to_string());
        let threshold: f32 = env::var("FACE_ONNX_MATCH_THRESHOLD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.36);

        if let Err(e) = Self::ensure_models(Path::new(&dir)).await {
            warn!("face_verify: model download failed ({e}); ONNX verification disabled");
            return None;
        }

        // Model loading is CPU-heavy; keep it off the async runtime.
        let dir_owned = dir.clone();
        let loaded = tokio::task::spawn_blocking(move || Self::load(Path::new(&dir_owned), threshold))
            .await
            .ok()?;
        match loaded {
            Ok(v) => {
                info!("face_verify: ONNX pipeline ready (UltraFace + ArcFace, threshold {threshold})");
                Some(v)
            }
            Err(e) => {
                warn!("face_verify: model load failed ({e}); ONNX verification disabled");
                None
            }
        }
    }

    /// Download any missing model file into `dir`.
    async fn ensure_models(dir: &Path) -> Result<(), String> {
        use std::env;
        tokio::fs::create_dir_all(dir).await.map_err(|e| e.to_string())?;
        let pairs = [
            (
                dir.join(DETECTOR_FILE),
                env::var("FACE_DETECTOR_URL").unwrap_or_else(|_| DEFAULT_DETECTOR_URL.into()),
            ),
            (
                dir.join(EMBEDDER_FILE),
                env::var("FACE_EMBEDDER_URL").unwrap_or_else(|_| DEFAULT_EMBEDDER_URL.into()),
            ),
        ];
        for (path, url) in pairs {
            if path.exists() {
                continue;
            }
            info!("face_verify: downloading {} ...", url);
            let bytes = reqwest::get(&url)
                .await
                .map_err(|e| e.to_string())?
                .error_for_status()
                .map_err(|e| e.to_string())?
                .bytes()
                .await
                .map_err(|e| e.to_string())?;
            tokio::fs::write(&path, &bytes).await.map_err(|e| e.to_string())?;
            info!("face_verify: saved {} ({} MB)", path.display(), bytes.len() / 1_048_576);
        }
        Ok(())
    }

    fn load(dir: &Path, match_threshold: f32) -> Result<Self, String> {
        let detector = Self::load_model(&dir.join(DETECTOR_FILE), [1, 3, DET_H, DET_W])?;
        let embedder = Self::load_model(&dir.join(EMBEDDER_FILE), [1, 3, EMB_SIZE, EMB_SIZE])?;
        Ok(Self { detector, embedder, match_threshold })
    }

    fn load_model(path: &Path, shape: [usize; 4]) -> Result<RunnableOnnx, String> {
        tract_onnx::onnx()
            .model_for_path(path)
            .and_then(|m| {
                m.with_input_fact(
                    0,
                    InferenceFact::dt_shape(
                        f32::datum_type(),
                        tvec!(shape[0], shape[1], shape[2], shape[3]),
                    ),
                )
            })
            .and_then(|m| m.into_optimized())
            .and_then(|m| m.into_runnable())
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Detect the largest face and return its L2-normalized 512-d ArcFace
    /// embedding. Ok(None) = no face found. Synchronous + CPU-bound: call via
    /// `tokio::task::spawn_blocking`.
    pub fn face_embedding(&self, image_bytes: &[u8]) -> Result<Option<Vec<f32>>, String> {
        let img = image::load_from_memory(image_bytes)
            .map_err(|e| format!("decode: {e}"))?
            .to_rgb8();
        let (ow, oh) = (img.width() as f32, img.height() as f32);

        // ── Detect ──────────────────────────────────────────────────────────
        // UltraFace: RGB, (x - 127) / 128, NCHW 240x320
        let resized = image::imageops::resize(
            &img,
            DET_W as u32,
            DET_H as u32,
            image::imageops::FilterType::Triangle,
        );
        let mut det_input = vec![0f32; 3 * DET_H * DET_W];
        for (x, y, p) in resized.enumerate_pixels() {
            let (x, y) = (x as usize, y as usize);
            for c in 0..3 {
                det_input[c * DET_H * DET_W + y * DET_W + x] = (p[c] as f32 - 127.0) / 128.0;
            }
        }
        let det_tensor: Tensor = Array4::from_shape_vec((1, 3, DET_H, DET_W), det_input)
            .map_err(|e| e.to_string())?
            .into();
        let det_out = self
            .detector
            .run(tvec!(det_tensor.into()))
            .map_err(|e| format!("detector: {e}"))?;
        // Outputs: scores (1, N, 2), boxes (1, N, 4) — boxes normalized x1,y1,x2,y2
        let scores = det_out[0]
            .to_array_view::<f32>()
            .map_err(|e| e.to_string())?
            .into_owned();
        let boxes = det_out[1]
            .to_array_view::<f32>()
            .map_err(|e| e.to_string())?
            .into_owned();
        let n = scores.shape()[1];

        // Largest face above the confidence floor
        let mut best: Option<(f32, [f32; 4])> = None; // (area, box)
        for i in 0..n {
            let score = scores[[0, i, 1]];
            if score < DET_MIN_SCORE {
                continue;
            }
            let b = [boxes[[0, i, 0]], boxes[[0, i, 1]], boxes[[0, i, 2]], boxes[[0, i, 3]]];
            let area = (b[2] - b[0]).max(0.0) * (b[3] - b[1]).max(0.0);
            if best.map(|(a, _)| area > a).unwrap_or(true) {
                best = Some((area, b));
            }
        }
        let Some((_, b)) = best else { return Ok(None) };

        // ── Crop with margin (normalized coords → original pixels) ──────────
        let (bw, bh) = (b[2] - b[0], b[3] - b[1]);
        let x1 = ((b[0] - bw * CROP_MARGIN) * ow).max(0.0) as u32;
        let y1 = ((b[1] - bh * CROP_MARGIN) * oh).max(0.0) as u32;
        let x2 = ((b[2] + bw * CROP_MARGIN) * ow).min(ow) as u32;
        let y2 = ((b[3] + bh * CROP_MARGIN) * oh).min(oh) as u32;
        if x2 <= x1 + 8 || y2 <= y1 + 8 {
            return Ok(None);
        }
        let face = image::imageops::crop_imm(&img, x1, y1, x2 - x1, y2 - y1).to_image();

        // ── Embed ───────────────────────────────────────────────────────────
        // ArcFace (ONNX zoo, MXNet export): RGB, raw 0..255 pixel values, NCHW
        let face = image::imageops::resize(
            &face,
            EMB_SIZE as u32,
            EMB_SIZE as u32,
            image::imageops::FilterType::Triangle,
        );
        let mut emb_input = vec![0f32; 3 * EMB_SIZE * EMB_SIZE];
        for (x, y, p) in face.enumerate_pixels() {
            let (x, y) = (x as usize, y as usize);
            for c in 0..3 {
                emb_input[c * EMB_SIZE * EMB_SIZE + y * EMB_SIZE + x] = p[c] as f32;
            }
        }
        let emb_tensor: Tensor = Array4::from_shape_vec((1, 3, EMB_SIZE, EMB_SIZE), emb_input)
            .map_err(|e| e.to_string())?
            .into();
        let emb_out = self
            .embedder
            .run(tvec!(emb_tensor.into()))
            .map_err(|e| format!("embedder: {e}"))?;
        let raw = emb_out[0]
            .to_array_view::<f32>()
            .map_err(|e| e.to_string())?;
        let mut v: Vec<f32> = raw.iter().copied().collect();

        // L2 normalize so cosine similarity is a plain dot product
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm <= f32::EPSILON {
            return Ok(None);
        }
        for x in &mut v {
            *x /= norm;
        }
        Ok(Some(v))
    }
}

/// Cosine similarity of two L2-normalized embeddings.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Path where models live (mirrors from_env logic, for diagnostics).
pub fn model_dir() -> PathBuf {
    PathBuf::from(std::env::var("VISION_MODEL_DIR").unwrap_or_else(|_| "models".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end discrimination check against real photos. Ignored by
    /// default (downloads ~250MB of models on first run). Usage:
    ///   FACE_TEST_A1=/path/personA_1.jpg FACE_TEST_A2=/path/personA_2.jpg \
    ///   FACE_TEST_B=/path/personB.jpg VISION_MODEL_DIR=/tmp/nava-models \
    ///   cargo test --bin telugu-dating-backend face_onnx_discrimination -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn face_onnx_discrimination() {
        let a1 = std::env::var("FACE_TEST_A1").expect("FACE_TEST_A1");
        let a2 = std::env::var("FACE_TEST_A2").expect("FACE_TEST_A2");
        let b = std::env::var("FACE_TEST_B").expect("FACE_TEST_B");
        std::env::set_var("FACE_VERIFY_ONNX", "1");

        let verifier = FaceVerifier::from_env().await.expect("verifier setup");
        let ea1 = verifier
            .face_embedding(&std::fs::read(a1).unwrap())
            .unwrap()
            .expect("face in A1");
        let ea2 = verifier
            .face_embedding(&std::fs::read(a2).unwrap())
            .unwrap()
            .expect("face in A2");
        let eb = verifier
            .face_embedding(&std::fs::read(b).unwrap())
            .unwrap()
            .expect("face in B");

        let same = cosine(&ea1, &ea2);
        let diff1 = cosine(&ea1, &eb);
        let diff2 = cosine(&ea2, &eb);
        println!("same-person cosine:      {same:.3}");
        println!("different-person cosine: {diff1:.3} / {diff2:.3}");
        println!("threshold:               {:.3}", verifier.match_threshold);

        assert!(same > verifier.match_threshold, "same person should match");
        assert!(diff1 < verifier.match_threshold, "different people should not match");
        assert!(diff2 < verifier.match_threshold, "different people should not match");
        assert!(same > diff1 + 0.15, "expected clear separation");
    }
}
