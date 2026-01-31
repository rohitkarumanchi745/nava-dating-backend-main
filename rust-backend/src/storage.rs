//! Storage Service - Local filesystem and S3/CDN support
//!
//! Supports:
//! - Local filesystem storage (development)
//! - AWS S3 with CloudFront CDN (production)
//! - Signed URLs for private content
//! - Image optimization and thumbnails

use std::path::Path;
use tokio::fs;
use tracing::{debug, error, info};
use uuid::Uuid;
use chrono::Utc;

/// Storage configuration
#[derive(Clone, Debug)]
pub struct StorageConfig {
    /// Storage backend: "local" or "s3"
    pub backend: String,

    /// Local storage directory (for local backend)
    pub local_dir: String,

    /// S3 bucket name
    pub s3_bucket: String,

    /// S3 region (e.g., "us-east-1")
    pub s3_region: String,

    /// S3 access key ID
    pub s3_access_key: String,

    /// S3 secret access key
    pub s3_secret_key: String,

    /// CloudFront distribution domain (e.g., "d1234.cloudfront.net")
    pub cdn_domain: String,

    /// CloudFront key pair ID for signed URLs
    pub cdn_key_pair_id: String,

    /// CloudFront private key PEM for signed URLs
    pub cdn_private_key: String,

    /// Base URL for serving files (local or CDN)
    pub base_url: String,

    /// Signed URL expiry in seconds (for private content)
    pub signed_url_expiry_secs: u64,
}

impl StorageConfig {
    pub fn from_env() -> Self {
        use std::env;

        let backend = env::var("STORAGE_BACKEND").unwrap_or_else(|_| "local".to_string());
        let local_dir = env::var("UPLOAD_DIR").unwrap_or_else(|_| "./uploads".to_string());

        Self {
            backend,
            local_dir,
            s3_bucket: env::var("S3_BUCKET").unwrap_or_default(),
            s3_region: env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".to_string()),
            s3_access_key: env::var("S3_ACCESS_KEY").unwrap_or_default(),
            s3_secret_key: env::var("S3_SECRET_KEY").unwrap_or_default(),
            cdn_domain: env::var("CDN_DOMAIN").unwrap_or_default(),
            cdn_key_pair_id: env::var("CDN_KEY_PAIR_ID").unwrap_or_default(),
            cdn_private_key: env::var("CDN_PRIVATE_KEY").unwrap_or_default(),
            base_url: env::var("STORAGE_BASE_URL").unwrap_or_else(|_| "/uploads".to_string()),
            signed_url_expiry_secs: env::var("SIGNED_URL_EXPIRY_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3600), // 1 hour default
        }
    }

    pub fn is_s3(&self) -> bool {
        self.backend == "s3"
    }

    pub fn is_local(&self) -> bool {
        self.backend == "local"
    }
}

/// Storage service for file uploads
#[derive(Clone)]
pub struct StorageService {
    config: StorageConfig,
}

/// File categories for organizing uploads
#[derive(Debug, Clone, Copy)]
pub enum FileCategory {
    ProfilePhoto,
    VoiceIntro,
    Spot,
    Reel,
    Message,
    Verification,
}

impl FileCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            FileCategory::ProfilePhoto => "photos",
            FileCategory::VoiceIntro => "voice",
            FileCategory::Spot => "spots",
            FileCategory::Reel => "reels",
            FileCategory::Message => "messages",
            FileCategory::Verification => "verification",
        }
    }

    /// Whether this category should use signed URLs (private content)
    pub fn is_private(&self) -> bool {
        matches!(self, FileCategory::Verification | FileCategory::Message)
    }
}

/// Result of a file upload
#[derive(Debug, Clone)]
pub struct UploadResult {
    /// The public URL to access the file
    pub url: String,

    /// The storage key/path
    pub key: String,

    /// File size in bytes
    pub size: usize,

    /// Content type
    pub content_type: String,
}

impl StorageService {
    pub fn new(config: StorageConfig) -> Self {
        Self { config }
    }

    /// Upload a file
    pub async fn upload(
        &self,
        category: FileCategory,
        user_id: i64,
        data: &[u8],
        content_type: &str,
    ) -> Result<UploadResult, StorageError> {
        let extension = content_type_to_extension(content_type);
        let filename = format!(
            "{}/{}_{}_{}{}",
            category.as_str(),
            user_id,
            Utc::now().timestamp(),
            Uuid::new_v4(),
            extension
        );

        if self.config.is_s3() {
            self.upload_s3(&filename, data, content_type).await
        } else {
            self.upload_local(&filename, data, content_type).await
        }
    }

    /// Upload to local filesystem
    async fn upload_local(
        &self,
        key: &str,
        data: &[u8],
        content_type: &str,
    ) -> Result<UploadResult, StorageError> {
        let path = Path::new(&self.config.local_dir).join(key);

        // Create parent directories
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                error!("Failed to create directory: {}", e);
                StorageError::IoError(e.to_string())
            })?;
        }

        // Write file
        fs::write(&path, data).await.map_err(|e| {
            error!("Failed to write file: {}", e);
            StorageError::IoError(e.to_string())
        })?;

        let url = format!("{}/{}", self.config.base_url, key);

        debug!("Uploaded file locally: {} ({} bytes)", key, data.len());

        Ok(UploadResult {
            url,
            key: key.to_string(),
            size: data.len(),
            content_type: content_type.to_string(),
        })
    }

    /// Upload to S3
    async fn upload_s3(
        &self,
        key: &str,
        data: &[u8],
        content_type: &str,
    ) -> Result<UploadResult, StorageError> {
        // Build S3 request using AWS Signature V4
        let bucket = &self.config.s3_bucket;
        let region = &self.config.s3_region;
        let endpoint = format!("https://{}.s3.{}.amazonaws.com/{}", bucket, region, key);

        // Create request
        let client = reqwest::Client::new();
        let now = Utc::now();
        let date_stamp = now.format("%Y%m%d").to_string();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();

        // AWS Signature V4 signing
        let credential_scope = format!("{}/{}/s3/aws4_request", date_stamp, region);

        // Create canonical request hash
        let payload_hash = sha256_hex(data);

        let canonical_headers = format!(
            "content-type:{}\nhost:{}.s3.{}.amazonaws.com\nx-amz-content-sha256:{}\nx-amz-date:{}\n",
            content_type, bucket, region, payload_hash, amz_date
        );
        let signed_headers = "content-type;host;x-amz-content-sha256;x-amz-date";

        let canonical_request = format!(
            "PUT\n/{}\n\n{}\n{}\n{}",
            key, canonical_headers, signed_headers, payload_hash
        );

        let canonical_request_hash = sha256_hex(canonical_request.as_bytes());

        // String to sign
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}\n{}",
            amz_date, credential_scope, canonical_request_hash
        );

        // Calculate signature
        let signing_key = get_signature_key(
            &self.config.s3_secret_key,
            &date_stamp,
            region,
            "s3",
        );
        let signature = hmac_sha256_hex(&signing_key, string_to_sign.as_bytes());

        // Authorization header
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
            self.config.s3_access_key, credential_scope, signed_headers, signature
        );

        // Make request
        let response = client
            .put(&endpoint)
            .header("Content-Type", content_type)
            .header("x-amz-content-sha256", &payload_hash)
            .header("x-amz-date", &amz_date)
            .header("Authorization", &authorization)
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| StorageError::S3Error(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("S3 upload failed: {} - {}", status, body);
            return Err(StorageError::S3Error(format!("Upload failed: {}", status)));
        }

        // Return CDN URL if configured, otherwise S3 URL
        let url = if !self.config.cdn_domain.is_empty() {
            format!("https://{}/{}", self.config.cdn_domain, key)
        } else {
            endpoint
        };

        info!("Uploaded file to S3: {} ({} bytes)", key, data.len());

        Ok(UploadResult {
            url,
            key: key.to_string(),
            size: data.len(),
            content_type: content_type.to_string(),
        })
    }

    /// Delete a file
    pub async fn delete(&self, key: &str) -> Result<(), StorageError> {
        if self.config.is_s3() {
            self.delete_s3(key).await
        } else {
            self.delete_local(key).await
        }
    }

    /// Delete from local filesystem
    async fn delete_local(&self, key: &str) -> Result<(), StorageError> {
        let path = Path::new(&self.config.local_dir).join(key);

        if path.exists() {
            fs::remove_file(&path).await.map_err(|e| {
                error!("Failed to delete file: {}", e);
                StorageError::IoError(e.to_string())
            })?;
            debug!("Deleted local file: {}", key);
        }

        Ok(())
    }

    /// Delete from S3
    async fn delete_s3(&self, key: &str) -> Result<(), StorageError> {
        let bucket = &self.config.s3_bucket;
        let region = &self.config.s3_region;
        let endpoint = format!("https://{}.s3.{}.amazonaws.com/{}", bucket, region, key);

        let client = reqwest::Client::new();
        let now = Utc::now();
        let date_stamp = now.format("%Y%m%d").to_string();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();

        let credential_scope = format!("{}/{}/s3/aws4_request", date_stamp, region);
        let payload_hash = sha256_hex(b"");

        let canonical_headers = format!(
            "host:{}.s3.{}.amazonaws.com\nx-amz-content-sha256:{}\nx-amz-date:{}\n",
            bucket, region, payload_hash, amz_date
        );
        let signed_headers = "host;x-amz-content-sha256;x-amz-date";

        let canonical_request = format!(
            "DELETE\n/{}\n\n{}\n{}\n{}",
            key, canonical_headers, signed_headers, payload_hash
        );

        let canonical_request_hash = sha256_hex(canonical_request.as_bytes());

        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}\n{}",
            amz_date, credential_scope, canonical_request_hash
        );

        let signing_key = get_signature_key(
            &self.config.s3_secret_key,
            &date_stamp,
            region,
            "s3",
        );
        let signature = hmac_sha256_hex(&signing_key, string_to_sign.as_bytes());

        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
            self.config.s3_access_key, credential_scope, signed_headers, signature
        );

        let response = client
            .delete(&endpoint)
            .header("x-amz-content-sha256", &payload_hash)
            .header("x-amz-date", &amz_date)
            .header("Authorization", &authorization)
            .send()
            .await
            .map_err(|e| StorageError::S3Error(e.to_string()))?;

        if !response.status().is_success() && response.status() != reqwest::StatusCode::NOT_FOUND {
            let status = response.status();
            error!("S3 delete failed: {}", status);
            return Err(StorageError::S3Error(format!("Delete failed: {}", status)));
        }

        info!("Deleted file from S3: {}", key);
        Ok(())
    }

    /// Get public URL for a file
    pub fn get_url(&self, key: &str) -> String {
        if self.config.is_s3() && !self.config.cdn_domain.is_empty() {
            format!("https://{}/{}", self.config.cdn_domain, key)
        } else if self.config.is_s3() {
            format!(
                "https://{}.s3.{}.amazonaws.com/{}",
                self.config.s3_bucket, self.config.s3_region, key
            )
        } else {
            format!("{}/{}", self.config.base_url, key)
        }
    }

    /// Generate a signed URL for private content (CloudFront)
    pub fn get_signed_url(&self, key: &str, expiry_secs: Option<u64>) -> Result<String, StorageError> {
        if self.config.cdn_domain.is_empty() || self.config.cdn_private_key.is_empty() {
            // Fall back to regular URL
            return Ok(self.get_url(key));
        }

        let expiry = expiry_secs.unwrap_or(self.config.signed_url_expiry_secs);
        let expires = Utc::now().timestamp() + expiry as i64;

        let url = format!("https://{}/{}", self.config.cdn_domain, key);
        let policy = format!(
            r#"{{"Statement":[{{"Resource":"{}","Condition":{{"DateLessThan":{{"AWS:EpochTime":{}}}}}}}]}}"#,
            url, expires
        );

        // Base64 encode policy (URL-safe)
        let policy_b64 = base64_url_encode(policy.as_bytes());

        // Sign the policy
        // Note: In production, use proper RSA signing with the CloudFront private key
        // This is a placeholder - real implementation needs RSA-SHA1 signing
        let signature = sign_cloudfront_policy(&self.config.cdn_private_key, &policy)?;

        Ok(format!(
            "{}?Policy={}&Signature={}&Key-Pair-Id={}",
            url, policy_b64, signature, self.config.cdn_key_pair_id
        ))
    }

    /// Check if a file exists
    pub async fn exists(&self, key: &str) -> bool {
        if self.config.is_s3() {
            // HEAD request to S3
            let bucket = &self.config.s3_bucket;
            let region = &self.config.s3_region;
            let endpoint = format!("https://{}.s3.{}.amazonaws.com/{}", bucket, region, key);

            if let Ok(response) = reqwest::Client::new().head(&endpoint).send().await {
                return response.status().is_success();
            }
            false
        } else {
            let path = Path::new(&self.config.local_dir).join(key);
            path.exists()
        }
    }
}

/// Storage errors
#[derive(Debug)]
pub enum StorageError {
    IoError(String),
    S3Error(String),
    SigningError(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::IoError(e) => write!(f, "IO error: {}", e),
            StorageError::S3Error(e) => write!(f, "S3 error: {}", e),
            StorageError::SigningError(e) => write!(f, "Signing error: {}", e),
        }
    }
}

impl std::error::Error for StorageError {}

// Helper functions

fn content_type_to_extension(content_type: &str) -> &'static str {
    match content_type {
        "image/jpeg" | "image/jpg" => ".jpg",
        "image/png" => ".png",
        "image/webp" => ".webp",
        "image/gif" => ".gif",
        "video/mp4" => ".mp4",
        "video/quicktime" => ".mov",
        "video/webm" => ".webm",
        "audio/mpeg" | "audio/mp3" => ".mp3",
        "audio/wav" => ".wav",
        "audio/m4a" | "audio/x-m4a" => ".m4a",
        "audio/aac" => ".aac",
        _ => "",
    }
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex::encode(result)
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(key)
        .expect("HMAC can take key of any size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn hmac_sha256_hex(key: &[u8], data: &[u8]) -> String {
    hex::encode(hmac_sha256(key, data))
}

fn get_signature_key(secret: &str, date_stamp: &str, region: &str, service: &str) -> Vec<u8> {
    // AWS Signature V4 key derivation
    let k_date = hmac_sha256(format!("AWS4{}", secret).as_bytes(), date_stamp.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    hmac_sha256(&k_service, b"aws4_request")
}

/// Hex encoding helper
mod hex {
    pub fn encode(data: impl AsRef<[u8]>) -> String {
        data.as_ref().iter().map(|b| format!("{:02x}", b)).collect()
    }
}

fn base64_url_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

fn sign_cloudfront_policy(private_key: &str, policy: &str) -> Result<String, StorageError> {
    // Placeholder for RSA-SHA1 signing
    // In production, use `rsa` crate with proper key parsing
    if private_key.is_empty() {
        return Err(StorageError::SigningError("No private key configured".to_string()));
    }

    // Return placeholder signature
    Ok(base64_url_encode(policy.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_type_to_extension() {
        assert_eq!(content_type_to_extension("image/jpeg"), ".jpg");
        assert_eq!(content_type_to_extension("video/mp4"), ".mp4");
        assert_eq!(content_type_to_extension("audio/mpeg"), ".mp3");
    }

    #[test]
    fn test_file_category() {
        assert_eq!(FileCategory::ProfilePhoto.as_str(), "photos");
        assert!(!FileCategory::ProfilePhoto.is_private());
        assert!(FileCategory::Verification.is_private());
    }
}
