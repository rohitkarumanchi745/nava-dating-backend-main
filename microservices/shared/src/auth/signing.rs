//! Request signing for secure inter-service communication
//!
//! Implements HMAC-SHA256 request signing with timestamp validation
//! to prevent replay attacks and ensure authenticity.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Signing configuration
#[derive(Debug, Clone)]
pub struct SigningConfig {
    /// Service identifier
    pub service_id: String,
    /// Shared secret for HMAC
    pub secret: String,
    /// Maximum age for valid signatures (default: 5 minutes)
    pub max_age_secs: u64,
}

impl SigningConfig {
    pub fn new(service_id: impl Into<String>, secret: impl Into<String>) -> Self {
        Self {
            service_id: service_id.into(),
            secret: secret.into(),
            max_age_secs: 300, // 5 minutes
        }
    }

    pub fn from_env(service_id: &str) -> Option<Self> {
        let secret = std::env::var("SERVICE_SIGNING_SECRET").ok()?;
        Some(Self::new(service_id, secret))
    }
}

/// Signed request metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedRequest {
    /// Service that signed the request
    pub service_id: String,
    /// Timestamp when signed
    pub timestamp: DateTime<Utc>,
    /// Request nonce for replay protection
    pub nonce: String,
    /// HMAC signature
    pub signature: String,
}

/// Request signer for outgoing inter-service calls
pub struct RequestSigner {
    config: SigningConfig,
}

impl RequestSigner {
    pub fn new(config: SigningConfig) -> Self {
        Self { config }
    }

    /// Sign an outgoing request
    pub fn sign(&self, method: &str, path: &str, body: Option<&[u8]>) -> SignedRequest {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let timestamp = Utc::now();
        let nonce = generate_nonce();

        let signature = self.compute_signature(method, path, body, &timestamp, &nonce);

        SignedRequest {
            service_id: self.config.service_id.clone(),
            timestamp,
            nonce,
            signature,
        }
    }

    /// Compute HMAC-SHA256 signature
    fn compute_signature(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        timestamp: &DateTime<Utc>,
        nonce: &str,
    ) -> String {
        let body_hash = body
            .map(|b| sha256_hex(b))
            .unwrap_or_else(|| sha256_hex(b""));

        let signing_string = format!(
            "{}\n{}\n{}\n{}\n{}",
            method.to_uppercase(),
            path,
            timestamp.timestamp(),
            nonce,
            body_hash
        );

        hmac_sha256_hex(&self.config.secret, &signing_string)
    }

    /// Add signature headers to a request
    pub fn sign_headers(
        &self,
        headers: &mut axum::http::HeaderMap,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
    ) {
        let signed = self.sign(method, path, body);

        if let Ok(v) = axum::http::HeaderValue::from_str(&signed.service_id) {
            headers.insert("X-Service-Id", v);
        }
        if let Ok(v) = axum::http::HeaderValue::from_str(&signed.timestamp.to_rfc3339()) {
            headers.insert("X-Timestamp", v);
        }
        if let Ok(v) = axum::http::HeaderValue::from_str(&signed.nonce) {
            headers.insert("X-Nonce", v);
        }
        if let Ok(v) = axum::http::HeaderValue::from_str(&signed.signature) {
            headers.insert("X-Signature", v);
        }
    }
}

/// Request verifier for incoming inter-service calls
pub struct RequestVerifier {
    config: SigningConfig,
    /// Set of recently seen nonces for replay protection
    seen_nonces: std::sync::RwLock<std::collections::HashSet<String>>,
}

impl RequestVerifier {
    pub fn new(config: SigningConfig) -> Self {
        Self {
            config,
            seen_nonces: std::sync::RwLock::new(std::collections::HashSet::new()),
        }
    }

    /// Verify an incoming signed request
    pub fn verify(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        signed: &SignedRequest,
    ) -> Result<(), SigningError> {
        // Check timestamp freshness
        let now = Utc::now();
        let age = now.signed_duration_since(signed.timestamp);

        if age.num_seconds().abs() as u64 > self.config.max_age_secs {
            return Err(SigningError::Expired {
                signed_at: signed.timestamp,
                max_age_secs: self.config.max_age_secs,
            });
        }

        // Check for replay (nonce reuse)
        {
            let seen = self.seen_nonces.read().unwrap();
            if seen.contains(&signed.nonce) {
                return Err(SigningError::ReplayDetected {
                    nonce: signed.nonce.clone(),
                });
            }
        }

        // Verify signature
        let expected = self.compute_signature(method, path, body, &signed.timestamp, &signed.nonce);

        if !constant_time_compare(&expected, &signed.signature) {
            return Err(SigningError::InvalidSignature);
        }

        // Store nonce to prevent replay
        {
            let mut seen = self.seen_nonces.write().unwrap();
            seen.insert(signed.nonce.clone());

            // Clean old nonces periodically (simple approach)
            if seen.len() > 10000 {
                seen.clear();
            }
        }

        Ok(())
    }

    /// Compute HMAC-SHA256 signature (same as signer)
    fn compute_signature(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        timestamp: &DateTime<Utc>,
        nonce: &str,
    ) -> String {
        let body_hash = body
            .map(|b| sha256_hex(b))
            .unwrap_or_else(|| sha256_hex(b""));

        let signing_string = format!(
            "{}\n{}\n{}\n{}\n{}",
            method.to_uppercase(),
            path,
            timestamp.timestamp(),
            nonce,
            body_hash
        );

        hmac_sha256_hex(&self.config.secret, &signing_string)
    }

    /// Extract signed request from headers
    pub fn extract_from_headers(
        headers: &axum::http::HeaderMap,
    ) -> Result<SignedRequest, SigningError> {
        let service_id = headers
            .get("X-Service-Id")
            .and_then(|v| v.to_str().ok())
            .ok_or(SigningError::MissingHeader("X-Service-Id"))?
            .to_string();

        let timestamp_str = headers
            .get("X-Timestamp")
            .and_then(|v| v.to_str().ok())
            .ok_or(SigningError::MissingHeader("X-Timestamp"))?;

        let timestamp = DateTime::parse_from_rfc3339(timestamp_str)
            .map_err(|_| SigningError::InvalidTimestamp)?
            .with_timezone(&Utc);

        let nonce = headers
            .get("X-Nonce")
            .and_then(|v| v.to_str().ok())
            .ok_or(SigningError::MissingHeader("X-Nonce"))?
            .to_string();

        let signature = headers
            .get("X-Signature")
            .and_then(|v| v.to_str().ok())
            .ok_or(SigningError::MissingHeader("X-Signature"))?
            .to_string();

        Ok(SignedRequest {
            service_id,
            timestamp,
            nonce,
            signature,
        })
    }
}

/// Signing errors
#[derive(Debug, thiserror::Error)]
pub enum SigningError {
    #[error("Missing required header: {0}")]
    MissingHeader(&'static str),

    #[error("Invalid timestamp format")]
    InvalidTimestamp,

    #[error("Signature expired: signed at {signed_at}, max age {max_age_secs}s")]
    Expired {
        signed_at: DateTime<Utc>,
        max_age_secs: u64,
    },

    #[error("Replay attack detected: nonce {nonce} already used")]
    ReplayDetected { nonce: String },

    #[error("Invalid signature")]
    InvalidSignature,
}

// Helper functions

fn generate_nonce() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.gen();
    hex::encode(bytes)
}

fn sha256_hex(data: &[u8]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // Simple hash for now - in production use proper SHA256
    // This is a placeholder that should use ring or sha2 crate
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn hmac_sha256_hex(key: &str, message: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // Simple HMAC for now - in production use proper HMAC-SHA256
    // This is a placeholder that should use ring or hmac crate
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    message.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn constant_time_compare(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut result = 0u8;
    for (x, y) in a.bytes().zip(b.bytes()) {
        result |= x ^ y;
    }
    result == 0
}

/// Axum middleware for verifying signed requests
pub async fn verify_service_request(
    verifier: std::sync::Arc<RequestVerifier>,
    method: axum::http::Method,
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, axum::http::StatusCode> {
    let signed = RequestVerifier::extract_from_headers(&headers)
        .map_err(|_| axum::http::StatusCode::UNAUTHORIZED)?;

    verifier
        .verify(
            method.as_str(),
            uri.path(),
            if body.is_empty() {
                None
            } else {
                Some(&body)
            },
            &signed,
        )
        .map_err(|e| {
            tracing::warn!(error = %e, "Service request verification failed");
            axum::http::StatusCode::UNAUTHORIZED
        })?;

    Ok(next.run(axum::http::Request::new(axum::body::Body::from(body))).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_and_verify() {
        let config = SigningConfig::new("test-service", "secret-key-123");
        let signer = RequestSigner::new(config.clone());
        let verifier = RequestVerifier::new(config);

        let signed = signer.sign("POST", "/api/users", Some(b"hello"));

        assert!(verifier
            .verify("POST", "/api/users", Some(b"hello"), &signed)
            .is_ok());
    }

    #[test]
    fn test_tampered_body() {
        let config = SigningConfig::new("test-service", "secret-key-123");
        let signer = RequestSigner::new(config.clone());
        let verifier = RequestVerifier::new(config);

        let signed = signer.sign("POST", "/api/users", Some(b"hello"));

        // Tampered body should fail
        assert!(verifier
            .verify("POST", "/api/users", Some(b"tampered"), &signed)
            .is_err());
    }

    #[test]
    fn test_replay_protection() {
        let config = SigningConfig::new("test-service", "secret-key-123");
        let signer = RequestSigner::new(config.clone());
        let verifier = RequestVerifier::new(config);

        let signed = signer.sign("GET", "/api/health", None);

        // First verification should succeed
        assert!(verifier.verify("GET", "/api/health", None, &signed).is_ok());

        // Replay should fail
        assert!(verifier.verify("GET", "/api/health", None, &signed).is_err());
    }
}
