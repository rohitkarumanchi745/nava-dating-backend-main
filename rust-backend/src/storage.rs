//! Storage Service - Local filesystem and S3-compatible object storage
//!
//! Supports:
//! - Local filesystem storage (development / single-node)
//! - Any S3-compatible store via AWS Signature V4: AWS S3, MinIO, Cloudflare R2
//!   (set S3_ENDPOINT for MinIO/R2; path-style addressing is used automatically)
//! - CloudFront CDN URLs and signed URLs for private content (AWS only)
//!
//! On Railway the container filesystem is ephemeral — every deploy wipes
//! uploads. Point STORAGE_BACKEND=s3 at a MinIO service (private network
//! endpoint) so media survives deploys and is shared across services.

use std::path::Path;
use tokio::fs;
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use chrono::Utc;

/// Storage configuration
#[derive(Clone, Debug)]
pub struct StorageConfig {
    /// Storage backend: "local" or "s3"
    pub backend: String,

    /// Local storage directory (for local backend)
    pub local_dir: String,

    /// Custom S3 endpoint, e.g. "http://minio.railway.internal:9000" for MinIO
    /// or "https://<account>.r2.cloudflarestorage.com" for R2. When set,
    /// path-style addressing ("{endpoint}/{bucket}/{key}") is used. When empty,
    /// AWS virtual-hosted style ("https://{bucket}.s3.{region}.amazonaws.com")
    /// is used.
    pub s3_endpoint: Option<String>,

    /// S3 bucket name
    pub s3_bucket: String,

    /// S3 region (MinIO accepts any; keep the default "us-east-1")
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

    /// Base URL path for serving files. Kept relative ("/uploads") so the
    /// stored DB paths keep working regardless of the API domain.
    pub base_url: String,

    /// Signed URL expiry in seconds (for private content)
    pub signed_url_expiry_secs: u64,
}

impl StorageConfig {
    pub fn from_env() -> Self {
        use std::env;

        let backend = env::var("STORAGE_BACKEND").unwrap_or_else(|_| "local".to_string());
        let local_dir = env::var("UPLOAD_DIR").unwrap_or_else(|_| "uploads".to_string());

        Self {
            backend,
            local_dir,
            s3_endpoint: env::var("S3_ENDPOINT")
                .ok()
                .map(|e| e.trim_end_matches('/').to_string())
                .filter(|e| !e.is_empty()),
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
    client: reqwest::Client,
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

/// A retrieved object ready for streaming to the client.
pub struct StoredObject {
    /// Underlying HTTP response from the object store (body not yet consumed)
    pub response: reqwest::Response,
    /// Whether this is a partial (206) response to a Range request
    pub partial: bool,
    pub content_type: Option<String>,
    pub content_length: Option<u64>,
    pub content_range: Option<String>,
}

impl StorageService {
    pub fn new(config: StorageConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    pub fn is_s3(&self) -> bool {
        self.config.is_s3()
    }

    pub fn local_dir(&self) -> &str {
        &self.config.local_dir
    }

    /// Upload a file with an auto-generated key
    pub async fn upload(
        &self,
        category: FileCategory,
        user_id: i64,
        data: &[u8],
        content_type: &str,
    ) -> Result<UploadResult, StorageError> {
        let extension = content_type_to_extension(content_type);
        let key = format!(
            "{}/{}_{}_{}{}",
            category.as_str(),
            user_id,
            Utc::now().timestamp(),
            Uuid::new_v4(),
            extension
        );
        self.put_key(&key, data, content_type).await
    }

    /// Upload a file under an exact key (e.g. "photos/123.jpg"). Existing
    /// handlers compute their own filenames and store "/uploads/{key}" in the
    /// DB — this preserves that contract in both backends.
    pub async fn put_key(
        &self,
        key: &str,
        data: &[u8],
        content_type: &str,
    ) -> Result<UploadResult, StorageError> {
        let key = key.trim_start_matches('/');
        if self.config.is_s3() {
            self.put_s3(key, data.to_vec(), content_type).await
        } else {
            self.put_local(key, data, content_type).await
        }
    }

    /// Like `put_key` but takes ownership of the buffer — no copy on the S3
    /// path. Use for large payloads (videos) to avoid double-buffering.
    pub async fn put_key_vec(
        &self,
        key: &str,
        data: Vec<u8>,
        content_type: &str,
    ) -> Result<UploadResult, StorageError> {
        let key = key.trim_start_matches('/');
        if self.config.is_s3() {
            self.put_s3(key, data, content_type).await
        } else {
            self.put_local(key, &data, content_type).await
        }
    }

    /// Read an object fully into memory (size-capped). Works in both backends;
    /// used by server-side jobs (e.g. selfie photo comparison).
    pub async fn get_bytes(&self, key: &str, max_bytes: usize) -> Option<Vec<u8>> {
        let key = key.trim_start_matches('/');
        if self.config.is_s3() {
            let resp = self.s3_request("GET", key, Vec::new(), None, None).await.ok()?;
            if !resp.status().is_success() {
                return None;
            }
            if let Some(len) = resp.content_length() {
                if len as usize > max_bytes {
                    return None;
                }
            }
            let bytes = resp.bytes().await.ok()?;
            if bytes.len() > max_bytes {
                return None;
            }
            Some(bytes.to_vec())
        } else {
            let path = Path::new(&self.config.local_dir).join(key);
            let bytes = fs::read(&path).await.ok()?;
            if bytes.len() > max_bytes {
                return None;
            }
            Some(bytes)
        }
    }

    /// Fetch an object for streaming to a client (S3 backend only — local mode
    /// serves straight from disk via ServeDir). `range` is an optional HTTP
    /// Range header value passed through so video seeking works.
    pub async fn get_object(
        &self,
        key: &str,
        range: Option<&str>,
    ) -> Result<Option<StoredObject>, StorageError> {
        let key = key.trim_start_matches('/');
        let resp = self.s3_request("GET", key, Vec::new(), None, range).await?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(StorageError::S3Error(format!("GET {key} failed: {status}")));
        }
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let content_range = resp
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        Ok(Some(StoredObject {
            partial: status == reqwest::StatusCode::PARTIAL_CONTENT,
            content_length: resp.content_length(),
            content_type,
            content_range,
            response: resp,
        }))
    }

    /// Upload every file under `local_dir` to the object store, preserving
    /// the directory structure below `key_prefix`. Used to publish ffmpeg's
    /// HLS output (segments + playlists) after a local transcode. No-op in
    /// local mode — the files are already where ServeDir serves them.
    /// Returns the number of files uploaded.
    pub async fn sync_dir(&self, local_dir: &str, key_prefix: &str) -> usize {
        if !self.config.is_s3() {
            return 0;
        }
        let root = std::path::PathBuf::from(local_dir);
        let mut stack = vec![root.clone()];
        let mut uploaded = 0usize;
        while let Some(dir) = stack.pop() {
            let Ok(mut entries) = fs::read_dir(&dir).await else { continue };
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
                if is_dir {
                    stack.push(path);
                    continue;
                }
                let Ok(bytes) = fs::read(&path).await else { continue };
                let rel = match path.strip_prefix(&root) {
                    Ok(r) => r.to_string_lossy().replace('\\', "/"),
                    Err(_) => continue,
                };
                if rel.is_empty() {
                    continue;
                }
                let key = format!("{}/{}", key_prefix.trim_matches('/'), rel);
                if self.put_key(&key, &bytes, guess_content_type(&key)).await.is_ok() {
                    uploaded += 1;
                } else {
                    warn!("sync_dir: failed to upload {key}");
                }
            }
        }
        uploaded
    }

    /// Create the bucket if it doesn't exist. Safe to call on every boot;
    /// MinIO/AWS return 409 for an already-owned bucket, which is ignored.
    pub async fn ensure_bucket(&self) {
        if !self.config.is_s3() {
            return;
        }
        match self.s3_request("PUT", "", Vec::new(), None, None).await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    info!("Created bucket {}", self.config.s3_bucket);
                } else if status == reqwest::StatusCode::CONFLICT {
                    debug!("Bucket {} already exists", self.config.s3_bucket);
                } else {
                    let body = resp.text().await.unwrap_or_default();
                    // BucketAlreadyOwnedByYou arrives as 409 on AWS but can be
                    // other shapes on MinIO variants; log and continue.
                    warn!("ensure_bucket: {status} {body}");
                }
            }
            Err(e) => warn!("ensure_bucket failed (uploads will error until fixed): {e}"),
        }
    }

    /// Upload to local filesystem
    async fn put_local(
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

    /// Upload to S3-compatible storage
    async fn put_s3(
        &self,
        key: &str,
        data: Vec<u8>,
        content_type: &str,
    ) -> Result<UploadResult, StorageError> {
        let size = data.len();
        let response = self
            .s3_request("PUT", key, data, Some(content_type), None)
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("S3 upload failed: {} - {}", status, body);
            return Err(StorageError::S3Error(format!("Upload failed: {}", status)));
        }

        info!("Uploaded file to S3: {} ({} bytes)", key, size);

        // Serve through the API's /uploads proxy so clients keep resolving the
        // same relative paths regardless of backend.
        Ok(UploadResult {
            url: format!("{}/{}", self.config.base_url, key),
            key: key.to_string(),
            size,
            content_type: content_type.to_string(),
        })
    }

    /// Delete a file
    pub async fn delete(&self, key: &str) -> Result<(), StorageError> {
        let key = key.trim_start_matches('/');
        if self.config.is_s3() {
            let response = self.s3_request("DELETE", key, Vec::new(), None, None).await?;
            if !response.status().is_success()
                && response.status() != reqwest::StatusCode::NOT_FOUND
            {
                let status = response.status();
                error!("S3 delete failed: {}", status);
                return Err(StorageError::S3Error(format!("Delete failed: {}", status)));
            }
            info!("Deleted file from S3: {}", key);
            Ok(())
        } else {
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
    }

    /// Get public URL for a file
    pub fn get_url(&self, key: &str) -> String {
        if self.config.is_s3() && !self.config.cdn_domain.is_empty() {
            format!("https://{}/{}", self.config.cdn_domain, key)
        } else {
            // Relative path served by the API (ServeDir locally, S3 proxy
            // otherwise) — stable across domains and backends.
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
        let key = key.trim_start_matches('/');
        if self.config.is_s3() {
            match self.s3_request("HEAD", key, Vec::new(), None, None).await {
                Ok(response) => response.status().is_success(),
                Err(_) => false,
            }
        } else {
            let path = Path::new(&self.config.local_dir).join(key);
            path.exists()
        }
    }

    // ------------------------------------------------------------------
    // AWS Signature V4 request builder (shared by all S3 operations)
    // ------------------------------------------------------------------

    /// Object URL + canonical URI for signing. With a custom endpoint (MinIO,
    /// R2) path-style is used; otherwise AWS virtual-hosted style.
    /// An empty `key` addresses the bucket itself (create-bucket).
    ///
    /// NOTE: keys are server-generated ([A-Za-z0-9_\-./]) so no URI escaping
    /// is applied; don't put user-supplied strings in keys.
    fn object_url(&self, key: &str) -> (String, String) {
        let bucket = &self.config.s3_bucket;
        if let Some(endpoint) = &self.config.s3_endpoint {
            let canonical = if key.is_empty() {
                format!("/{}", bucket)
            } else {
                format!("/{}/{}", bucket, key)
            };
            (format!("{}{}", endpoint, canonical), canonical)
        } else {
            let region = &self.config.s3_region;
            let canonical = format!("/{}", key);
            (
                format!("https://{}.s3.{}.amazonaws.com/{}", bucket, region, key),
                canonical,
            )
        }
    }

    /// Build, sign (SigV4) and execute an S3 request. `range` is sent as an
    /// unsigned header (allowed by SigV4 — only headers listed in
    /// SignedHeaders participate in the signature).
    async fn s3_request(
        &self,
        method: &str,
        key: &str,
        body: Vec<u8>,
        content_type: Option<&str>,
        range: Option<&str>,
    ) -> Result<reqwest::Response, StorageError> {
        let (url, canonical_uri) = self.object_url(key);
        let parsed = reqwest::Url::parse(&url)
            .map_err(|e| StorageError::S3Error(format!("bad S3 url: {e}")))?;
        // Host header must include a non-default port (MinIO's :9000)
        let host = match (parsed.host_str(), parsed.port()) {
            (Some(h), Some(p)) => format!("{}:{}", h, p),
            (Some(h), None) => h.to_string(),
            _ => return Err(StorageError::S3Error("S3 url missing host".into())),
        };

        let now = Utc::now();
        let date_stamp = now.format("%Y%m%d").to_string();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let region = &self.config.s3_region;
        let credential_scope = format!("{}/{}/s3/aws4_request", date_stamp, region);
        let payload_hash = sha256_hex(&body);

        // Canonical headers, alphabetically ordered
        let (canonical_headers, signed_headers) = match content_type {
            Some(ct) => (
                format!(
                    "content-type:{}\nhost:{}\nx-amz-content-sha256:{}\nx-amz-date:{}\n",
                    ct, host, payload_hash, amz_date
                ),
                "content-type;host;x-amz-content-sha256;x-amz-date",
            ),
            None => (
                format!(
                    "host:{}\nx-amz-content-sha256:{}\nx-amz-date:{}\n",
                    host, payload_hash, amz_date
                ),
                "host;x-amz-content-sha256;x-amz-date",
            ),
        };

        let canonical_request = format!(
            "{}\n{}\n\n{}\n{}\n{}",
            method, canonical_uri, canonical_headers, signed_headers, payload_hash
        );

        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}\n{}",
            amz_date,
            credential_scope,
            sha256_hex(canonical_request.as_bytes())
        );

        let signing_key =
            get_signature_key(&self.config.s3_secret_key, &date_stamp, region, "s3");
        let signature = hmac_sha256_hex(&signing_key, string_to_sign.as_bytes());

        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
            self.config.s3_access_key, credential_scope, signed_headers, signature
        );

        let mut req = match method {
            "PUT" => self.client.put(&url),
            "GET" => self.client.get(&url),
            "DELETE" => self.client.delete(&url),
            "HEAD" => self.client.head(&url),
            other => return Err(StorageError::S3Error(format!("unsupported method {other}"))),
        };
        req = req
            .header("x-amz-content-sha256", &payload_hash)
            .header("x-amz-date", &amz_date)
            .header("Authorization", &authorization);
        if let Some(ct) = content_type {
            req = req.header("Content-Type", ct);
        }
        if let Some(r) = range {
            req = req.header("Range", r);
        }
        if !body.is_empty() {
            req = req.body(body);
        }

        req.send()
            .await
            .map_err(|e| StorageError::S3Error(e.to_string()))
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

/// Guess a Content-Type from a key's extension (for serving objects stored
/// without one, or by older code paths).
pub fn guess_content_type(key: &str) -> &'static str {
    let ext = key.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/m4a",
        "aac" => "audio/aac",
        // HLS artifacts
        "m3u8" => "application/vnd.apple.mpegurl",
        "ts" => "video/mp2t",
        _ => "application/octet-stream",
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

    #[test]
    fn test_guess_content_type() {
        assert_eq!(guess_content_type("photos/a.jpg"), "image/jpeg");
        assert_eq!(guess_content_type("reels/v.mp4"), "video/mp4");
        assert_eq!(guess_content_type("weird"), "application/octet-stream");
    }

    #[test]
    fn test_object_url_path_style_with_endpoint() {
        let mut cfg = StorageConfig::from_env();
        cfg.backend = "s3".into();
        cfg.s3_endpoint = Some("http://minio.railway.internal:9000".into());
        cfg.s3_bucket = "nava-media".into();
        let svc = StorageService::new(cfg);
        let (url, canonical) = svc.object_url("photos/1.jpg");
        assert_eq!(url, "http://minio.railway.internal:9000/nava-media/photos/1.jpg");
        assert_eq!(canonical, "/nava-media/photos/1.jpg");
        // Empty key addresses the bucket (create-bucket)
        let (burl, bcanon) = svc.object_url("");
        assert_eq!(burl, "http://minio.railway.internal:9000/nava-media");
        assert_eq!(bcanon, "/nava-media");
    }

    /// End-to-end SigV4 check against a real MinIO. Ignored by default; run:
    ///   docker run -d --rm -p 19000:9000 -e MINIO_ROOT_USER=testkey \
    ///     -e MINIO_ROOT_PASSWORD=testsecret123 minio/minio server /data
    ///   cargo test --bin telugu-dating-backend minio_roundtrip -- --ignored
    #[tokio::test]
    #[ignore]
    async fn minio_roundtrip() {
        let mut cfg = StorageConfig::from_env();
        cfg.backend = "s3".into();
        cfg.s3_endpoint = Some(
            std::env::var("MINIO_TEST_ENDPOINT")
                .unwrap_or_else(|_| "http://127.0.0.1:19000".into()),
        );
        cfg.s3_bucket = "nava-test".into();
        cfg.s3_access_key = "testkey".into();
        cfg.s3_secret_key = "testsecret123".into();
        let svc = StorageService::new(cfg);

        svc.ensure_bucket().await;

        let body = b"hello nava".to_vec();
        let res = svc
            .put_key("photos/it_test.jpg", &body, "image/jpeg")
            .await
            .expect("put failed");
        assert_eq!(res.url, "/uploads/photos/it_test.jpg");

        assert!(svc.exists("photos/it_test.jpg").await, "HEAD after PUT");

        let bytes = svc
            .get_bytes("photos/it_test.jpg", 1024)
            .await
            .expect("get_bytes failed");
        assert_eq!(bytes, body);

        // Range request (video seeking path)
        let obj = svc
            .get_object("photos/it_test.jpg", Some("bytes=0-4"))
            .await
            .expect("get_object errored")
            .expect("object missing");
        assert!(obj.partial, "expected 206 for range request");
        assert_eq!(obj.response.bytes().await.unwrap().as_ref(), b"hello");

        // Missing object → None, not error
        assert!(svc.get_object("photos/nope.jpg", None).await.unwrap().is_none());

        svc.delete("photos/it_test.jpg").await.expect("delete failed");
        assert!(!svc.exists("photos/it_test.jpg").await, "gone after DELETE");
    }

    #[test]
    fn test_object_url_virtual_hosted_aws() {
        let mut cfg = StorageConfig::from_env();
        cfg.backend = "s3".into();
        cfg.s3_endpoint = None;
        cfg.s3_bucket = "b".into();
        cfg.s3_region = "us-east-1".into();
        let svc = StorageService::new(cfg);
        let (url, canonical) = svc.object_url("k/x.png");
        assert_eq!(url, "https://b.s3.us-east-1.amazonaws.com/k/x.png");
        assert_eq!(canonical, "/k/x.png");
    }
}
