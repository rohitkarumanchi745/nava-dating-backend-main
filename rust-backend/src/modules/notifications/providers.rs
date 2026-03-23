//! Notification providers for different channels
//!
//! Implements push notifications (FCM/APNs), email (SMTP), and SMS (Twilio).

use std::sync::Arc;
use tracing::{error, info, warn};

use super::push::{DeviceRegistry, Platform, PushNotificationService, PushPayload};

// ============================================
// Unified Push Provider (FCM + APNs)
// ============================================

pub struct PushProvider {
    service: Arc<PushNotificationService>,
}

impl PushProvider {
    pub async fn new(pool: sqlx::PgPool) -> Self {
        let service = PushNotificationService::new(pool).await;

        // Ensure device tokens table exists
        if let Err(e) = service.device_registry().ensure_table().await {
            error!(error = %e, "Failed to ensure device tokens table");
        }

        Self {
            service: Arc::new(service),
        }
    }

    /// Send push notification to a user (all registered devices).
    /// `data` may include a `"category"` key — if present it is lifted into
    /// `aps.category` so iOS triggers the matching UNNotificationCategory action set.
    pub async fn send(
        &self,
        user_id: i32,
        title: &str,
        body: &str,
        data: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        let mut payload = PushPayload::new(title, body);

        if let Some(d) = data {
            payload = payload.with_data(d.clone());
            // Lift "category" from data into aps.category for actionable notifications
            if let Some(cat) = d.get("category").and_then(|v| v.as_str()) {
                payload = payload.with_category(cat);
            }
        }

        let results = self.service.send_to_user(user_id, &payload).await;

        let success_count = results.iter().filter(|r| r.success).count();
        let total = results.len();

        if total == 0 {
            warn!(user_id, "No devices registered for push notification");
            return Ok(());
        }

        if success_count == 0 {
            return Err(format!("All {} push notifications failed", total));
        }

        info!(
            user_id,
            success = success_count,
            total,
            "Push notifications sent"
        );

        Ok(())
    }

    /// Send push notification to a specific device token
    pub async fn send_to_token(
        &self,
        token: &str,
        platform: &str,
        title: &str,
        body: &str,
        data: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        let platform: Platform = platform.parse()?;

        let device = super::push::DeviceToken {
            id: 0,
            user_id: 0,
            token: token.to_string(),
            platform,
            device_id: None,
            app_version: None,
            is_active: true,
            last_used_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let mut payload = PushPayload::new(title, body);
        if let Some(d) = data {
            payload = payload.with_data(d.clone());
        }

        let result = self.service.send_to_device(&device, &payload).await;

        if result.success {
            Ok(())
        } else {
            Err(result.error.map(|e| e.to_string()).unwrap_or_default())
        }
    }

    /// Register a device token for a user
    pub async fn register_device(
        &self,
        user_id: i32,
        token: &str,
        platform: &str,
        device_id: Option<&str>,
    ) -> Result<(), String> {
        let platform: Platform = platform.parse()?;
        self.service
            .register_device(user_id, token, platform, device_id)
            .await
    }

    /// Unregister a device token
    pub async fn unregister_device(&self, token: &str) -> Result<(), String> {
        self.service.unregister_device(token).await
    }

    /// Get device registry for direct access
    pub fn device_registry(&self) -> Arc<DeviceRegistry> {
        self.service.device_registry()
    }
}

// ============================================
// Email Provider (SMTP)
// ============================================

pub struct EmailProvider {
    smtp_host: Option<String>,
    smtp_port: u16,
    smtp_user: Option<String>,
    smtp_password: Option<String>,
    from_address: String,
    from_name: String,
}

impl EmailProvider {
    pub fn new() -> Self {
        Self {
            smtp_host: std::env::var("SMTP_HOST").ok(),
            smtp_port: std::env::var("SMTP_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(587),
            smtp_user: std::env::var("SMTP_USER").ok(),
            smtp_password: std::env::var("SMTP_PASSWORD").ok(),
            from_address: std::env::var("SMTP_FROM")
                .unwrap_or_else(|_| "noreply@nava.app".to_string()),
            from_name: std::env::var("SMTP_FROM_NAME")
                .unwrap_or_else(|_| "NAVA".to_string()),
        }
    }

    pub fn is_configured(&self) -> bool {
        self.smtp_host.is_some()
    }

    pub async fn send(
        &self,
        user_id: i32,
        template: &str,
        subject: &str,
        _variables: &serde_json::Value,
    ) -> Result<(), String> {
        let Some(ref _host) = self.smtp_host else {
            warn!(user_id, "SMTP not configured, skipping email");
            return Ok(());
        };

        info!(
            user_id,
            template,
            subject,
            "Sending email"
        );

        // TODO: Implement actual SMTP integration with templates
        // 1. Look up user's email from database
        // 2. Load and render template with variables
        // 3. Send via SMTP using lettre crate

        Ok(())
    }

    pub async fn send_to_address(
        &self,
        to_email: &str,
        subject: &str,
        _html_body: &str,
        _text_body: Option<&str>,
    ) -> Result<(), String> {
        let Some(ref _host) = self.smtp_host else {
            return Err("SMTP not configured".to_string());
        };

        info!(
            to = %to_email,
            subject,
            "Sending email to address"
        );

        // In production, use the `lettre` crate:
        //
        // let email = Message::builder()
        //     .from(format!("{} <{}>", self.from_name, self.from_address).parse().unwrap())
        //     .to(to_email.parse().unwrap())
        //     .subject(subject)
        //     .multipart(
        //         MultiPart::alternative()
        //             .singlepart(SinglePart::plain(text_body.unwrap_or("")))
        //             .singlepart(SinglePart::html(html_body))
        //     )
        //     .unwrap();
        //
        // let creds = Credentials::new(self.smtp_user.clone(), self.smtp_password.clone());
        // let mailer = SmtpTransport::relay(host)
        //     .unwrap()
        //     .port(self.smtp_port)
        //     .credentials(creds)
        //     .build();
        //
        // mailer.send(&email).map_err(|e| e.to_string())

        Ok(())
    }
}

// ============================================
// SMS Provider (Twilio)
// ============================================

pub struct SmsProvider {
    account_sid: Option<String>,
    auth_token: Option<String>,
    from_number: Option<String>,
    http_client: reqwest::Client,
}

impl SmsProvider {
    pub fn new() -> Self {
        Self {
            account_sid: std::env::var("TWILIO_ACCOUNT_SID").ok(),
            auth_token: std::env::var("TWILIO_AUTH_TOKEN").ok(),
            from_number: std::env::var("TWILIO_PHONE_NUMBER").ok(),
            http_client: reqwest::Client::new(),
        }
    }

    pub fn is_configured(&self) -> bool {
        self.account_sid.is_some() && self.auth_token.is_some() && self.from_number.is_some()
    }

    pub async fn send(&self, phone_number: &str, message: &str) -> Result<(), String> {
        let (Some(ref account_sid), Some(ref auth_token), Some(ref from_number)) =
            (&self.account_sid, &self.auth_token, &self.from_number)
        else {
            warn!(phone = %phone_number, "Twilio not configured, skipping SMS");
            return Ok(());
        };

        info!(
            phone = %phone_number,
            message_length = message.len(),
            "Sending SMS"
        );

        let url = format!(
            "https://api.twilio.com/2010-04-01/Accounts/{}/Messages.json",
            account_sid
        );

        let params = [
            ("To", phone_number),
            ("From", from_number),
            ("Body", message),
        ];

        let response = self
            .http_client
            .post(&url)
            .basic_auth(account_sid, Some(auth_token))
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("Twilio request failed: {}", e))?;

        if response.status().is_success() {
            info!(phone = %phone_number, "SMS sent successfully");
            Ok(())
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!(status = %status, body = %body, "Twilio request failed");
            Err(format!("Twilio returned {}: {}", status, body))
        }
    }

    /// Send OTP via SMS
    pub async fn send_otp(&self, phone_number: &str, otp: &str) -> Result<(), String> {
        let message = format!("Your NAVA verification code is: {}. This code expires in 10 minutes.", otp);
        self.send(phone_number, &message).await
    }
}

// ============================================
// In-App Notification Provider
// ============================================

pub struct InAppProvider {
    pool: sqlx::PgPool,
}

impl InAppProvider {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    pub async fn ensure_table(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS in_app_notifications (
                id BIGSERIAL PRIMARY KEY,
                user_id INTEGER NOT NULL,
                title VARCHAR(255) NOT NULL,
                body TEXT NOT NULL,
                notification_type VARCHAR(50) NOT NULL,
                data JSONB,
                is_read BOOLEAN NOT NULL DEFAULT false,
                read_at TIMESTAMPTZ,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_in_app_notifications_user_unread
            ON in_app_notifications (user_id, is_read) WHERE is_read = false
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn create(
        &self,
        user_id: i32,
        title: &str,
        body: &str,
        notification_type: &str,
        data: Option<&serde_json::Value>,
    ) -> Result<i64, String> {
        let result: (i64,) = sqlx::query_as(
            r#"
            INSERT INTO in_app_notifications (user_id, title, body, notification_type, data)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id
            "#,
        )
        .bind(user_id)
        .bind(title)
        .bind(body)
        .bind(notification_type)
        .bind(data)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| format!("Failed to create in-app notification: {}", e))?;

        Ok(result.0)
    }

    pub async fn mark_as_read(&self, notification_id: i64, user_id: i32) -> Result<bool, String> {
        let result = sqlx::query(
            r#"
            UPDATE in_app_notifications
            SET is_read = true, read_at = NOW()
            WHERE id = $1 AND user_id = $2
            "#,
        )
        .bind(notification_id)
        .bind(user_id)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to mark notification as read: {}", e))?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn mark_all_as_read(&self, user_id: i32) -> Result<u64, String> {
        let result = sqlx::query(
            r#"
            UPDATE in_app_notifications
            SET is_read = true, read_at = NOW()
            WHERE user_id = $1 AND is_read = false
            "#,
        )
        .bind(user_id)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to mark all notifications as read: {}", e))?;

        Ok(result.rows_affected())
    }

    pub async fn get_unread_count(&self, user_id: i32) -> Result<i64, String> {
        let count: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*) FROM in_app_notifications
            WHERE user_id = $1 AND is_read = false
            "#,
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| format!("Failed to get unread count: {}", e))?;

        Ok(count.0)
    }
}
