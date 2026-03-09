//! Apple Push Notification service (APNs) provider for iOS push notifications
//!
//! Uses APNs HTTP/2 API with JWT-based authentication.
//! https://developer.apple.com/documentation/usernotifications/setting_up_a_remote_notification_server

use super::{PushError, PushPayload, PushResult};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::sync::RwLock;
use tracing::{error, info};

/// APNs production endpoint
const APNS_PRODUCTION_URL: &str = "https://api.push.apple.com";
/// APNs development/sandbox endpoint
const APNS_SANDBOX_URL: &str = "https://api.sandbox.push.apple.com";

/// APNs provider configuration
#[derive(Debug, Clone)]
pub struct ApnsConfig {
    /// Apple Team ID (10 character string)
    pub team_id: String,
    /// APNs Key ID (10 character string)
    pub key_id: String,
    /// Private key (.p8 file contents)
    pub private_key: String,
    /// App bundle identifier
    pub bundle_id: String,
    /// Use sandbox/development environment
    pub sandbox: bool,
}

impl ApnsConfig {
    pub fn from_env() -> Result<Self, String> {
        let team_id = std::env::var("APNS_TEAM_ID")
            .map_err(|_| "APNS_TEAM_ID not set")?;

        let key_id = std::env::var("APNS_KEY_ID")
            .map_err(|_| "APNS_KEY_ID not set")?;

        let bundle_id = std::env::var("APNS_BUNDLE_ID")
            .or_else(|_| std::env::var("IOS_BUNDLE_ID"))
            .map_err(|_| "APNS_BUNDLE_ID not set")?;

        // Read private key from file or environment
        let private_key = if let Ok(path) = std::env::var("APNS_KEY_PATH") {
            std::fs::read_to_string(&path)
                .map_err(|e| format!("Failed to read APNs key file: {}", e))?
        } else if let Ok(key) = std::env::var("APNS_PRIVATE_KEY") {
            key
        } else {
            return Err("APNS_KEY_PATH or APNS_PRIVATE_KEY not set".to_string());
        };

        let sandbox = std::env::var("APNS_SANDBOX")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        Ok(Self {
            team_id,
            key_id,
            private_key,
            bundle_id,
            sandbox,
        })
    }
}

/// JWT token cache for APNs
struct JwtCache {
    token: String,
    expires_at: DateTime<Utc>,
}

/// Apple Push Notification service provider
pub struct ApnsProvider {
    config: ApnsConfig,
    http_client: reqwest::Client,
    jwt_cache: RwLock<Option<JwtCache>>,
}

impl ApnsProvider {
    pub async fn new(config: ApnsConfig) -> Result<Self, String> {
        // Create HTTP/2 client
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("Failed to create HTTP/2 client: {}", e))?;

        let provider = Self {
            config,
            http_client,
            jwt_cache: RwLock::new(None),
        };

        // Verify we can create a JWT
        provider.get_jwt()?;

        info!(
            sandbox = provider.config.sandbox,
            bundle_id = %provider.config.bundle_id,
            "APNs provider initialized successfully"
        );

        Ok(provider)
    }

    pub async fn from_env() -> Result<Self, String> {
        let config = ApnsConfig::from_env()?;
        Self::new(config).await
    }

    /// Get or create JWT for APNs authentication
    fn get_jwt(&self) -> Result<String, String> {
        // Check cache first
        {
            let cache = self.jwt_cache.read().unwrap();
            if let Some(ref cached) = *cache {
                // JWT valid for 1 hour, refresh after 50 minutes
                if cached.expires_at > Utc::now() {
                    return Ok(cached.token.clone());
                }
            }
        }

        // Create new JWT
        let jwt = self.create_jwt()?;

        // Cache it
        {
            let mut cache = self.jwt_cache.write().unwrap();
            *cache = Some(JwtCache {
                token: jwt.clone(),
                expires_at: Utc::now() + Duration::minutes(50),
            });
        }

        Ok(jwt)
    }

    /// Create APNs JWT token
    fn create_jwt(&self) -> Result<String, String> {
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

        let now = Utc::now();

        // JWT Header
        let header = serde_json::json!({
            "alg": "ES256",
            "kid": self.config.key_id
        });
        let header_b64 = URL_SAFE_NO_PAD.encode(header.to_string());

        // JWT Claims
        let claims = serde_json::json!({
            "iss": self.config.team_id,
            "iat": now.timestamp()
        });
        let claims_b64 = URL_SAFE_NO_PAD.encode(claims.to_string());

        // Message to sign
        let message = format!("{}.{}", header_b64, claims_b64);

        // Sign with ES256 (ECDSA with P-256 curve and SHA-256)
        let signature = self.sign_es256(&message)?;
        let signature_b64 = URL_SAFE_NO_PAD.encode(&signature);

        Ok(format!("{}.{}", message, signature_b64))
    }

    /// Sign message with ES256 (ECDSA P-256)
    fn sign_es256(&self, message: &str) -> Result<Vec<u8>, String> {
        // In production, use the `ring` or `p256` crate for proper ECDSA signing
        // This is a simplified implementation using OpenSSL
        use std::process::Command;

        let key_file = "/tmp/apns_key.p8";
        let sig_file = "/tmp/apns_sig.bin";

        // Write private key to temp file
        std::fs::write(key_file, &self.config.private_key)
            .map_err(|e| format!("Failed to write key file: {}", e))?;

        // Use OpenSSL to sign with ECDSA
        let _output = Command::new("sh")
            .arg("-c")
            .arg(format!(
                "echo -n '{}' | openssl dgst -sha256 -sign {} | openssl asn1parse -inform DER -out {} 2>/dev/null || echo -n '{}' | openssl dgst -sha256 -sign {} -out {}",
                message, key_file, sig_file, message, key_file, sig_file
            ))
            .output()
            .map_err(|e| format!("OpenSSL signing failed: {}", e))?;

        // Try to read signature
        let signature = std::fs::read(sig_file)
            .map_err(|e| format!("Failed to read signature: {}", e))?;

        // Clean up
        let _ = std::fs::remove_file(key_file);
        let _ = std::fs::remove_file(sig_file);

        // Convert DER signature to raw r||s format (64 bytes)
        let raw_sig = self.der_to_raw_signature(&signature)?;

        Ok(raw_sig)
    }

    /// Convert DER-encoded ECDSA signature to raw r||s format
    fn der_to_raw_signature(&self, der: &[u8]) -> Result<Vec<u8>, String> {
        // Simple DER parsing for ECDSA signature
        // Format: 0x30 len 0x02 r_len r 0x02 s_len s

        if der.len() < 8 || der[0] != 0x30 {
            return Err("Invalid DER signature format".to_string());
        }

        let mut idx = 2; // Skip 0x30 and length byte

        // Parse r
        if der[idx] != 0x02 {
            return Err("Invalid DER: expected INTEGER".to_string());
        }
        idx += 1;
        let r_len = der[idx] as usize;
        idx += 1;
        let r_start = idx;
        let r_end = r_start + r_len;
        idx = r_end;

        // Parse s
        if der[idx] != 0x02 {
            return Err("Invalid DER: expected INTEGER".to_string());
        }
        idx += 1;
        let s_len = der[idx] as usize;
        idx += 1;
        let s_start = idx;
        let s_end = s_start + s_len;

        // Extract r and s, padding/trimming to 32 bytes each
        let r = &der[r_start..r_end];
        let s = &der[s_start..s_end];

        let mut raw = vec![0u8; 64];

        // Copy r (right-aligned, skip leading zeros if > 32 bytes)
        let r_offset = if r.len() > 32 { r.len() - 32 } else { 0 };
        let r_dest = if r.len() < 32 { 32 - r.len() } else { 0 };
        raw[r_dest..32].copy_from_slice(&r[r_offset..]);

        // Copy s (right-aligned, skip leading zeros if > 32 bytes)
        let s_offset = if s.len() > 32 { s.len() - 32 } else { 0 };
        let s_dest = if s.len() < 32 { 64 - s.len() } else { 32 };
        raw[s_dest..64].copy_from_slice(&s[s_offset..]);

        Ok(raw)
    }

    /// Send push notification via APNs
    pub async fn send(&self, device_token: &str, payload: &PushPayload) -> PushResult {
        let jwt = match self.get_jwt() {
            Ok(token) => token,
            Err(e) => return PushResult::failure(PushError::AuthError(e)),
        };

        // Build APNs payload
        let apns_payload = ApnsPayload {
            aps: ApsPayload {
                alert: Some(ApsAlert {
                    title: payload.title.clone(),
                    body: payload.body.clone(),
                }),
                badge: payload.badge,
                sound: payload.sound.clone(),
                category: payload.category.clone(),
                mutable_content: Some(1),
                content_available: Some(1),
            },
            data: if payload.data.is_null() {
                None
            } else {
                Some(payload.data.clone())
            },
        };

        let base_url = if self.config.sandbox {
            APNS_SANDBOX_URL
        } else {
            APNS_PRODUCTION_URL
        };

        let url = format!("{}/3/device/{}", base_url, device_token);

        let response = match self
            .http_client
            .post(&url)
            .header("authorization", format!("bearer {}", jwt))
            .header("apns-topic", &self.config.bundle_id)
            .header("apns-push-type", "alert")
            .header("apns-priority", "10")
            .header("apns-expiration", "0")
            .json(&apns_payload)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => return PushResult::failure(PushError::NetworkError(e.to_string())),
        };

        let status = response.status();

        // Get apns-id header
        let apns_id = response
            .headers()
            .get("apns-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        if status.is_success() {
            info!(token = %device_token, "APNs notification sent successfully");
            PushResult::success(apns_id.unwrap_or_default())
        } else {
            let body = response.text().await.unwrap_or_default();
            self.handle_apns_error(status, &body)
        }
    }

    fn handle_apns_error(&self, status: reqwest::StatusCode, body: &str) -> PushResult {
        let error_response: Result<ApnsErrorResponse, _> = serde_json::from_str(body);

        let error = if let Ok(err) = error_response {
            match err.reason.as_str() {
                "BadDeviceToken" | "DeviceTokenNotForTopic" => PushError::InvalidToken,
                "Unregistered" => PushError::Unregistered,
                "ExpiredToken" => PushError::ExpiredToken,
                "TooManyRequests" => PushError::RateLimited,
                "PayloadTooLarge" => PushError::PayloadTooLarge,
                "ExpiredProviderToken" | "InvalidProviderToken" => {
                    // Clear JWT cache to force refresh
                    {
                        let mut cache = self.jwt_cache.write().unwrap();
                        *cache = None;
                    }
                    PushError::AuthError(err.reason)
                }
                _ => PushError::ServerError(err.reason),
            }
        } else {
            PushError::ServerError(format!("HTTP {}: {}", status, body))
        };

        error!(status = %status, body = %body, "APNs request failed");
        PushResult::failure(error)
    }
}

// ============================================
// Data Structures
// ============================================

#[derive(Serialize)]
struct ApnsPayload {
    aps: ApsPayload,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct ApsPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    alert: Option<ApsAlert>,
    #[serde(skip_serializing_if = "Option::is_none")]
    badge: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sound: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    category: Option<String>,
    #[serde(rename = "mutable-content", skip_serializing_if = "Option::is_none")]
    mutable_content: Option<u8>,
    #[serde(rename = "content-available", skip_serializing_if = "Option::is_none")]
    content_available: Option<u8>,
}

#[derive(Serialize)]
struct ApsAlert {
    title: String,
    body: String,
}

#[derive(Deserialize)]
struct ApnsErrorResponse {
    reason: String,
}

// ============================================
// Silent Push Notifications
// ============================================

impl ApnsProvider {
    /// Send silent push notification (background update)
    pub async fn send_silent(&self, device_token: &str, data: serde_json::Value) -> PushResult {
        let jwt = match self.get_jwt() {
            Ok(token) => token,
            Err(e) => return PushResult::failure(PushError::AuthError(e)),
        };

        let apns_payload = serde_json::json!({
            "aps": {
                "content-available": 1
            },
            "data": data
        });

        let base_url = if self.config.sandbox {
            APNS_SANDBOX_URL
        } else {
            APNS_PRODUCTION_URL
        };

        let url = format!("{}/3/device/{}", base_url, device_token);

        let response = match self
            .http_client
            .post(&url)
            .header("authorization", format!("bearer {}", jwt))
            .header("apns-topic", &self.config.bundle_id)
            .header("apns-push-type", "background")
            .header("apns-priority", "5") // Lower priority for background
            .json(&apns_payload)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => return PushResult::failure(PushError::NetworkError(e.to_string())),
        };

        let status = response.status();
        let apns_id = response
            .headers()
            .get("apns-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        if status.is_success() {
            info!(token = %device_token, "APNs silent notification sent");
            PushResult::success(apns_id.unwrap_or_default())
        } else {
            let body = response.text().await.unwrap_or_default();
            self.handle_apns_error(status, &body)
        }
    }
}
