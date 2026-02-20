//! Push notification providers
//!
//! Platform-specific push notification implementations:
//! - Firebase Cloud Messaging (FCM) for Android
//! - Apple Push Notification service (APNs) for iOS

pub mod apns;
pub mod fcm;
pub mod device_registry;

pub use apns::ApnsProvider;
pub use fcm::FcmProvider;
pub use device_registry::{DeviceRegistry, DeviceToken, Platform};

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info, warn};

/// Unified push notification payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushPayload {
    pub title: String,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub badge: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sound: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default)]
    pub data: serde_json::Value,
}

impl PushPayload {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            image_url: None,
            badge: None,
            sound: Some("default".to_string()),
            category: None,
            data: serde_json::json!({}),
        }
    }

    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = data;
        self
    }

    pub fn with_badge(mut self, badge: u32) -> Self {
        self.badge = Some(badge);
        self
    }

    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = Some(category.into());
        self
    }

    pub fn silent(mut self) -> Self {
        self.sound = None;
        self
    }
}

/// Push notification result
#[derive(Debug)]
pub struct PushResult {
    pub success: bool,
    pub message_id: Option<String>,
    pub error: Option<PushError>,
    pub should_remove_token: bool,
}

impl PushResult {
    pub fn success(message_id: String) -> Self {
        Self {
            success: true,
            message_id: Some(message_id),
            error: None,
            should_remove_token: false,
        }
    }

    pub fn failure(error: PushError) -> Self {
        let should_remove = matches!(
            error,
            PushError::InvalidToken | PushError::Unregistered | PushError::ExpiredToken
        );
        Self {
            success: false,
            message_id: None,
            error: Some(error),
            should_remove_token: should_remove,
        }
    }
}

/// Push notification errors
#[derive(Debug, thiserror::Error)]
pub enum PushError {
    #[error("Invalid device token")]
    InvalidToken,

    #[error("Device unregistered")]
    Unregistered,

    #[error("Token expired")]
    ExpiredToken,

    #[error("Rate limited")]
    RateLimited,

    #[error("Payload too large")]
    PayloadTooLarge,

    #[error("Server error: {0}")]
    ServerError(String),

    #[error("Authentication error: {0}")]
    AuthError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Provider not configured")]
    NotConfigured,
}

/// Unified push notification service
pub struct PushNotificationService {
    fcm: Option<FcmProvider>,
    apns: Option<ApnsProvider>,
    device_registry: Arc<DeviceRegistry>,
}

impl PushNotificationService {
    pub async fn new(pool: sqlx::PgPool) -> Self {
        let fcm = FcmProvider::from_env().await.ok();
        let apns = ApnsProvider::from_env().await.ok();

        if fcm.is_none() {
            warn!("FCM provider not configured - Android push notifications disabled");
        }
        if apns.is_none() {
            warn!("APNs provider not configured - iOS push notifications disabled");
        }

        Self {
            fcm,
            apns,
            device_registry: Arc::new(DeviceRegistry::new(pool)),
        }
    }

    /// Send push notification to a user on all their registered devices
    pub async fn send_to_user(&self, user_id: i32, payload: &PushPayload) -> Vec<PushResult> {
        let devices = match self.device_registry.get_user_devices(user_id).await {
            Ok(devices) => devices,
            Err(e) => {
                error!(user_id, error = %e, "Failed to get user devices");
                return vec![];
            }
        };

        if devices.is_empty() {
            warn!(user_id, "No registered devices for user");
            return vec![];
        }

        let mut results = Vec::new();

        for device in devices {
            let result = self.send_to_device(&device, payload).await;

            // Remove invalid tokens
            if result.should_remove_token {
                if let Err(e) = self.device_registry.remove_device(&device.token).await {
                    error!(token = %device.token, error = %e, "Failed to remove invalid token");
                }
            }

            results.push(result);
        }

        results
    }

    /// Send push notification to a specific device
    pub async fn send_to_device(&self, device: &DeviceToken, payload: &PushPayload) -> PushResult {
        match device.platform {
            Platform::Android => {
                if let Some(ref fcm) = self.fcm {
                    fcm.send(&device.token, payload).await
                } else {
                    PushResult::failure(PushError::NotConfigured)
                }
            }
            Platform::Ios => {
                if let Some(ref apns) = self.apns {
                    apns.send(&device.token, payload).await
                } else {
                    PushResult::failure(PushError::NotConfigured)
                }
            }
        }
    }

    /// Register a device token for a user
    pub async fn register_device(
        &self,
        user_id: i32,
        token: &str,
        platform: Platform,
        device_id: Option<&str>,
    ) -> Result<(), String> {
        self.device_registry
            .register_device(user_id, token, platform, device_id)
            .await
    }

    /// Unregister a device token
    pub async fn unregister_device(&self, token: &str) -> Result<(), String> {
        self.device_registry.remove_device(token).await
    }

    /// Get device registry for direct access
    pub fn device_registry(&self) -> Arc<DeviceRegistry> {
        self.device_registry.clone()
    }
}
