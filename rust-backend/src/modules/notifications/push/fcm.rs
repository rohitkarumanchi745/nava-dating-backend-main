//! Firebase Cloud Messaging (FCM) provider for Android push notifications
//!
//! Uses FCM HTTP v1 API with service account authentication.
//! https://firebase.google.com/docs/cloud-messaging/http-server-ref

use super::{PushError, PushPayload, PushResult};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::sync::RwLock;
use tracing::{debug, error, info};

/// FCM HTTP v1 API endpoint
const FCM_API_URL: &str = "https://fcm.googleapis.com/v1/projects/{project_id}/messages:send";

/// FCM provider configuration
#[derive(Debug, Clone)]
pub struct FcmConfig {
    /// Firebase project ID
    pub project_id: String,
    /// Service account credentials JSON
    pub service_account_json: String,
}

impl FcmConfig {
    pub fn from_env() -> Result<Self, String> {
        let project_id = std::env::var("FIREBASE_PROJECT_ID")
            .map_err(|_| "FIREBASE_PROJECT_ID not set")?;

        // Try to read from file or environment variable
        let service_account_json = if let Ok(path) = std::env::var("GOOGLE_APPLICATION_CREDENTIALS") {
            std::fs::read_to_string(&path)
                .map_err(|e| format!("Failed to read service account file: {}", e))?
        } else if let Ok(json) = std::env::var("FIREBASE_SERVICE_ACCOUNT_JSON") {
            json
        } else {
            return Err("GOOGLE_APPLICATION_CREDENTIALS or FIREBASE_SERVICE_ACCOUNT_JSON not set".to_string());
        };

        Ok(Self {
            project_id,
            service_account_json,
        })
    }
}

/// OAuth2 access token cache
struct TokenCache {
    access_token: String,
    expires_at: DateTime<Utc>,
}

/// Firebase Cloud Messaging provider
pub struct FcmProvider {
    config: FcmConfig,
    http_client: reqwest::Client,
    token_cache: RwLock<Option<TokenCache>>,
}

impl FcmProvider {
    pub async fn new(config: FcmConfig) -> Result<Self, String> {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

        let provider = Self {
            config,
            http_client,
            token_cache: RwLock::new(None),
        };

        // Verify credentials by getting initial token
        provider.get_access_token().await?;

        info!("FCM provider initialized successfully");
        Ok(provider)
    }

    pub async fn from_env() -> Result<Self, String> {
        let config = FcmConfig::from_env()?;
        Self::new(config).await
    }

    /// Get OAuth2 access token (cached)
    async fn get_access_token(&self) -> Result<String, String> {
        // Check cache first
        {
            let cache = self.token_cache.read().unwrap();
            if let Some(ref cached) = *cache {
                // Return cached token if not expired (with 5 min buffer)
                if cached.expires_at > Utc::now() + Duration::minutes(5) {
                    return Ok(cached.access_token.clone());
                }
            }
        }

        // Need to refresh token
        let token = self.fetch_access_token().await?;

        // Cache the new token
        {
            let mut cache = self.token_cache.write().unwrap();
            *cache = Some(TokenCache {
                access_token: token.clone(),
                expires_at: Utc::now() + Duration::minutes(55), // Token valid for 1 hour
            });
        }

        Ok(token)
    }

    /// Fetch new OAuth2 access token from Google
    async fn fetch_access_token(&self) -> Result<String, String> {
        // Parse service account JSON
        let service_account: ServiceAccount = serde_json::from_str(&self.config.service_account_json)
            .map_err(|e| format!("Invalid service account JSON: {}", e))?;

        // Create JWT for token request
        let now = Utc::now();
        let jwt_claims = JwtClaims {
            iss: service_account.client_email.clone(),
            scope: "https://www.googleapis.com/auth/firebase.messaging".to_string(),
            aud: "https://oauth2.googleapis.com/token".to_string(),
            iat: now.timestamp(),
            exp: (now + Duration::minutes(60)).timestamp(),
        };

        // Sign JWT with private key
        let jwt = self.create_jwt(&jwt_claims, &service_account.private_key)?;

        // Exchange JWT for access token
        let response = self
            .http_client
            .post("https://oauth2.googleapis.com/token")
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &jwt),
            ])
            .send()
            .await
            .map_err(|e| format!("Token request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Token request failed with {}: {}", status, body));
        }

        let token_response: TokenResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse token response: {}", e))?;

        debug!("Obtained FCM access token");
        Ok(token_response.access_token)
    }

    /// Create a signed JWT
    fn create_jwt(&self, claims: &JwtClaims, private_key: &str) -> Result<String, String> {
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

        // JWT Header
        let header = r#"{"alg":"RS256","typ":"JWT"}"#;
        let header_b64 = URL_SAFE_NO_PAD.encode(header);

        // JWT Claims
        let claims_json = serde_json::to_string(claims)
            .map_err(|e| format!("Failed to serialize claims: {}", e))?;
        let claims_b64 = URL_SAFE_NO_PAD.encode(&claims_json);

        // Message to sign
        let message = format!("{}.{}", header_b64, claims_b64);

        // Sign with RSA-SHA256
        let signature = self.sign_rs256(&message, private_key)?;
        let signature_b64 = URL_SAFE_NO_PAD.encode(&signature);

        Ok(format!("{}.{}", message, signature_b64))
    }

    /// Sign message with RSA-SHA256
    fn sign_rs256(&self, message: &str, private_key_pem: &str) -> Result<Vec<u8>, String> {
        // In production, use the `ring` or `rsa` crate for proper RSA signing
        // This is a simplified implementation
        use std::process::Command;

        // Create temp files for signing (not ideal, but works for demo)
        let key_file = "/tmp/fcm_key.pem";
        let sig_file = "/tmp/fcm_sig.bin";

        std::fs::write(key_file, private_key_pem)
            .map_err(|e| format!("Failed to write key file: {}", e))?;

        // Use OpenSSL to sign
        let output = Command::new("sh")
            .arg("-c")
            .arg(format!(
                "echo -n '{}' | openssl dgst -sha256 -sign {} -out {}",
                message, key_file, sig_file
            ))
            .output()
            .map_err(|e| format!("OpenSSL signing failed: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "OpenSSL signing failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let signature = std::fs::read(sig_file)
            .map_err(|e| format!("Failed to read signature: {}", e))?;

        // Clean up
        let _ = std::fs::remove_file(key_file);
        let _ = std::fs::remove_file(sig_file);

        Ok(signature)
    }

    /// Send push notification via FCM
    pub async fn send(&self, device_token: &str, payload: &PushPayload) -> PushResult {
        let access_token = match self.get_access_token().await {
            Ok(token) => token,
            Err(e) => return PushResult::failure(PushError::AuthError(e)),
        };

        let fcm_message = FcmMessage {
            message: FcmMessageBody {
                token: device_token.to_string(),
                notification: Some(FcmNotification {
                    title: payload.title.clone(),
                    body: payload.body.clone(),
                    image: payload.image_url.clone(),
                }),
                android: Some(FcmAndroidConfig {
                    priority: "high".to_string(),
                    notification: Some(FcmAndroidNotification {
                        sound: payload.sound.clone().unwrap_or_else(|| "default".to_string()),
                        channel_id: payload.category.clone().unwrap_or_else(|| "default".to_string()),
                        click_action: Some("FLUTTER_NOTIFICATION_CLICK".to_string()),
                    }),
                }),
                data: if payload.data.is_null() {
                    None
                } else {
                    Some(payload.data.clone())
                },
            },
        };

        let url = FCM_API_URL.replace("{project_id}", &self.config.project_id);

        let response = match self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/json")
            .json(&fcm_message)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => return PushResult::failure(PushError::NetworkError(e.to_string())),
        };

        let status = response.status();

        if status.is_success() {
            let result: FcmResponse = response.json().await.unwrap_or(FcmResponse { name: None });
            info!(token = %device_token, "FCM notification sent successfully");
            PushResult::success(result.name.unwrap_or_default())
        } else {
            let body = response.text().await.unwrap_or_default();
            self.handle_fcm_error(status, &body)
        }
    }

    /// Send to multiple devices (batch)
    pub async fn send_multicast(
        &self,
        device_tokens: &[String],
        payload: &PushPayload,
    ) -> Vec<PushResult> {
        let mut results = Vec::with_capacity(device_tokens.len());

        // FCM HTTP v1 doesn't support multicast, so we send individually
        // Consider using FCM legacy API or Topics for true multicast
        for token in device_tokens {
            results.push(self.send(token, payload).await);
        }

        results
    }

    fn handle_fcm_error(&self, status: reqwest::StatusCode, body: &str) -> PushResult {
        // Parse FCM error response
        let error_response: Result<FcmErrorResponse, _> = serde_json::from_str(body);

        let error = if let Ok(err) = error_response {
            match err.error.code.as_str() {
                "INVALID_ARGUMENT" => {
                    if body.contains("not a valid FCM registration token") {
                        PushError::InvalidToken
                    } else {
                        PushError::ServerError(err.error.message)
                    }
                }
                "NOT_FOUND" => PushError::Unregistered,
                "RESOURCE_EXHAUSTED" => PushError::RateLimited,
                "UNAUTHENTICATED" => PushError::AuthError(err.error.message),
                "UNAVAILABLE" | "INTERNAL" => PushError::ServerError(err.error.message),
                _ => PushError::ServerError(format!("{}: {}", err.error.code, err.error.message)),
            }
        } else {
            PushError::ServerError(format!("HTTP {}: {}", status, body))
        };

        error!(status = %status, body = %body, "FCM request failed");
        PushResult::failure(error)
    }
}

// ============================================
// Data Structures
// ============================================

#[derive(Deserialize)]
struct ServiceAccount {
    client_email: String,
    private_key: String,
}

#[derive(Serialize)]
struct JwtClaims {
    iss: String,
    scope: String,
    aud: String,
    iat: i64,
    exp: i64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Serialize)]
struct FcmMessage {
    message: FcmMessageBody,
}

#[derive(Serialize)]
struct FcmMessageBody {
    token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    notification: Option<FcmNotification>,
    #[serde(skip_serializing_if = "Option::is_none")]
    android: Option<FcmAndroidConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct FcmNotification {
    title: String,
    body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<String>,
}

#[derive(Serialize)]
struct FcmAndroidConfig {
    priority: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    notification: Option<FcmAndroidNotification>,
}

#[derive(Serialize)]
struct FcmAndroidNotification {
    sound: String,
    channel_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    click_action: Option<String>,
}

#[derive(Deserialize)]
struct FcmResponse {
    name: Option<String>,
}

#[derive(Deserialize)]
struct FcmErrorResponse {
    error: FcmErrorDetails,
}

#[derive(Deserialize)]
struct FcmErrorDetails {
    code: String,
    message: String,
}
