//! AWS Rekognition client — SigV4-signed JSON calls, no AWS SDK dependency.
//!
//! Powers the authoritative anti-fraud checks that on-device ML cannot be
//! trusted with (client-asserted scores are spoofable):
//! - `compare_faces`: selfie ↔ profile-photo identity match (/verify/selfie)
//! - `recognize_celebrities`: celebrity / stolen-photo detection at upload
//!
//! Reuses the same AWS credentials as media storage (S3_ACCESS_KEY /
//! S3_SECRET_KEY / S3_REGION). The IAM policy must additionally allow
//! `rekognition:CompareFaces` and `rekognition:RecognizeCelebrities`
//! (Resource "*" — Rekognition has no resource-level scoping for these).
//!
//! Images are sent as raw bytes (JPEG/PNG, ≤ 5MB) — profile photos are
//! pipeline-encoded JPEGs well under that. Cost: ~$0.001 per image.

use base64::Engine;
use chrono::Utc;
use serde_json::{json, Value};

use crate::storage::{get_signature_key, hmac_sha256_hex, sha256_hex};

/// A celebrity recognized in a photo.
#[derive(Debug, Clone)]
pub struct CelebrityHit {
    pub name: String,
    /// Match confidence 0..100.
    pub confidence: f32,
}

#[derive(Clone)]
pub struct Rekognition {
    client: reqwest::Client,
    region: String,
    access_key: String,
    secret_key: String,
}

impl Rekognition {
    /// Build from the storage AWS credentials. Returns None when the keys are
    /// absent or REKOGNITION_ENABLED=false — callers treat None as "feature
    /// unavailable" (verify falls back, screening is skipped).
    pub fn from_env() -> Option<Self> {
        use std::env;
        let enabled = env::var("REKOGNITION_ENABLED")
            .map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(true);
        if !enabled {
            return None;
        }
        let access_key = env::var("S3_ACCESS_KEY").ok().filter(|s| !s.is_empty())?;
        let secret_key = env::var("S3_SECRET_KEY").ok().filter(|s| !s.is_empty())?;
        let region = env::var("REKOGNITION_REGION")
            .or_else(|_| env::var("S3_REGION"))
            .unwrap_or_else(|_| "us-east-1".to_string());
        Some(Self {
            client: reqwest::Client::new(),
            region,
            access_key,
            secret_key,
        })
    }

    /// Highest face similarity (0..100) between the source (selfie) and the
    /// target (profile photo). `Ok(None)` means Rekognition found no matching
    /// face at or above `min_similarity`.
    pub async fn compare_faces(
        &self,
        source_image: &[u8],
        target_image: &[u8],
        min_similarity: f32,
    ) -> Result<Option<f32>, String> {
        let b64 = base64::engine::general_purpose::STANDARD;
        let body = json!({
            "SourceImage": { "Bytes": b64.encode(source_image) },
            "TargetImage": { "Bytes": b64.encode(target_image) },
            "SimilarityThreshold": min_similarity,
        });
        let out = self.call("RekognitionService.CompareFaces", &body).await?;
        let best = out["FaceMatches"]
            .as_array()
            .and_then(|arr| {
                arr.iter()
                    .filter_map(|m| m["Similarity"].as_f64())
                    .fold(None, |acc: Option<f64>, v| {
                        Some(acc.map_or(v, |a| a.max(v)))
                    })
            })
            .map(|v| v as f32);
        Ok(best)
    }

    /// Celebrities recognized in the image, with 0..100 match confidence.
    pub async fn recognize_celebrities(&self, image: &[u8]) -> Result<Vec<CelebrityHit>, String> {
        let b64 = base64::engine::general_purpose::STANDARD;
        let body = json!({ "Image": { "Bytes": b64.encode(image) } });
        let out = self
            .call("RekognitionService.RecognizeCelebrities", &body)
            .await?;
        Ok(out["CelebrityFaces"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| {
                        Some(CelebrityHit {
                            name: c["Name"].as_str()?.to_string(),
                            confidence: c["MatchConfidence"].as_f64()? as f32,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Sign (SigV4, service "rekognition") and execute one JSON-1.1 API call.
    async fn call(&self, target: &str, body: &Value) -> Result<Value, String> {
        let host = format!("rekognition.{}.amazonaws.com", self.region);
        let url = format!("https://{}/", host);
        let payload = serde_json::to_vec(body).map_err(|e| e.to_string())?;
        const CONTENT_TYPE: &str = "application/x-amz-json-1.1";

        let now = Utc::now();
        let date_stamp = now.format("%Y%m%d").to_string();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let credential_scope = format!("{}/{}/rekognition/aws4_request", date_stamp, self.region);
        let payload_hash = sha256_hex(&payload);

        // Canonical headers, alphabetically ordered
        let canonical_headers = format!(
            "content-type:{}\nhost:{}\nx-amz-date:{}\nx-amz-target:{}\n",
            CONTENT_TYPE, host, amz_date, target
        );
        let signed_headers = "content-type;host;x-amz-date;x-amz-target";
        let canonical_request = format!(
            "POST\n/\n\n{}\n{}\n{}",
            canonical_headers, signed_headers, payload_hash
        );
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}\n{}",
            amz_date,
            credential_scope,
            sha256_hex(canonical_request.as_bytes())
        );
        let signing_key =
            get_signature_key(&self.secret_key, &date_stamp, &self.region, "rekognition");
        let signature = hmac_sha256_hex(&signing_key, string_to_sign.as_bytes());
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
            self.access_key, credential_scope, signed_headers, signature
        );

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", CONTENT_TYPE)
            .header("X-Amz-Date", &amz_date)
            .header("X-Amz-Target", target)
            .header("Authorization", &authorization)
            .body(payload)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let status = resp.status();
        let text = resp.text().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!("{status}: {text}"));
        }
        serde_json::from_str(&text).map_err(|e| e.to_string())
    }
}
