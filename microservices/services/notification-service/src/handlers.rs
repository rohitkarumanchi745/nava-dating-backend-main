//! Notification handlers for different event types
//!
//! Contains business logic for sending notifications via different channels.

use std::sync::Arc;

use chrono::{Timelike, Utc};
use sqlx::PgPool;
use tracing::{error, info, warn};

use crate::policy::{GateDecision, NotifCategory, NotificationPolicy};
use crate::providers::{EmailProvider, InAppProvider, PushProvider, SmsProvider};

pub struct NotificationHandlers {
    pool: Option<PgPool>,
    push: PushProvider,
    email: EmailProvider,
    sms: SmsProvider,
    in_app: Option<InAppProvider>,
    policy: Arc<NotificationPolicy>,
}

impl NotificationHandlers {
    pub async fn new(pool: Option<PgPool>, policy: Arc<NotificationPolicy>) -> Self {
        let push = if let Some(ref p) = pool {
            PushProvider::new(p.clone()).await
        } else {
            // Create without database (limited functionality)
            warn!("No database pool provided - push notifications will have limited functionality");
            PushProvider::new(
                sqlx::PgPool::connect("postgres://localhost/nava")
                    .await
                    .expect("Failed to connect to database"),
            )
            .await
        };

        let in_app = if let Some(ref p) = pool {
            let provider = InAppProvider::new(p.clone());
            if let Err(e) = provider.ensure_table().await {
                error!(error = %e, "Failed to ensure in_app_notifications table");
            }
            Some(provider)
        } else {
            None
        };

        Self {
            pool: pool.clone(),
            push,
            email: EmailProvider::new(),
            sms: SmsProvider::new(),
            in_app,
            policy,
        }
    }

    /// Run a notification through the policy gate and send if approved.
    /// Returns Ok(true) if sent, Ok(false) if suppressed/deferred.
    async fn gated_send(
        &self,
        user_id: i32,
        category: NotifCategory,
        utc_offset_hours: i32,
        data: Option<&serde_json::Value>,
    ) -> Result<bool, String> {
        let decision = self.policy.gate(user_id, category, utc_offset_hours).await;

        match decision {
            GateDecision::Send { variant, bandit_selected } => {
                let sent_hour = ((Utc::now().hour() as i32 + utc_offset_hours).rem_euclid(24)) as u8;

                // Send push
                self.push.send(user_id, &variant.title, &variant.body, data).await?;

                // In-app
                if let Some(ref in_app) = self.in_app {
                    let _ = in_app.create(
                        user_id, &variant.title, &variant.body,
                        category.as_str(), data,
                    ).await;
                }

                // Log for offline eval
                self.policy.log_outcome(
                    user_id, category.as_str(), &variant.variant_id,
                    bandit_selected, sent_hour,
                ).await;

                info!(
                    user_id,
                    category = category.as_str(),
                    variant = %variant.variant_id,
                    bandit = bandit_selected,
                    "Notification sent via policy"
                );

                Ok(true)
            }
            GateDecision::Defer { optimal_hour, reason } => {
                info!(
                    user_id,
                    category = category.as_str(),
                    optimal_hour,
                    reason = %reason,
                    "Notification deferred"
                );
                // TODO: Enqueue for later delivery at optimal_hour
                Ok(false)
            }
            GateDecision::Suppress { reason } => {
                info!(
                    user_id,
                    category = category.as_str(),
                    reason = %reason,
                    "Notification suppressed"
                );
                Ok(false)
            }
        }
    }

    // ============================================
    // Device Registration
    // ============================================

    /// Register a device token for push notifications
    pub async fn register_device(
        &self,
        user_id: i32,
        token: &str,
        platform: &str,
        device_id: Option<&str>,
    ) -> Result<(), String> {
        self.push
            .register_device(user_id, token, platform, device_id)
            .await
    }

    /// Unregister a device token
    pub async fn unregister_device(&self, token: &str) -> Result<(), String> {
        self.push.unregister_device(token).await
    }

    // ============================================
    // User Event Handlers
    // ============================================

    pub async fn send_welcome_notification(&self, user_id: i32) -> Result<(), String> {
        // Send push notification
        self.push
            .send(
                user_id,
                "Welcome to Nava!",
                "Start your journey to find meaningful connections.",
                None,
            )
            .await?;

        // Also create in-app notification
        if let Some(ref in_app) = self.in_app {
            in_app
                .create(
                    user_id,
                    "Welcome to Nava!",
                    "Complete your profile to start matching with people.",
                    "welcome",
                    None,
                )
                .await?;
        }

        Ok(())
    }

    pub async fn send_verification_success(
        &self,
        user_id: i32,
        verification_type: &str,
    ) -> Result<(), String> {
        let (title, message) = match verification_type {
            "phone" => ("Phone Verified", "Your phone number has been verified!"),
            "email" => ("Email Verified", "Your email has been verified!"),
            "selfie" => (
                "Selfie Verified",
                "Your selfie verification is complete. Your profile is now verified!",
            ),
            "student" => (
                "Student Verified",
                "Your student status has been verified!",
            ),
            _ => ("Verification Complete", "Your account has been verified!"),
        };

        self.push.send(user_id, title, message, None).await?;

        if let Some(ref in_app) = self.in_app {
            in_app
                .create(user_id, title, message, "verification", None)
                .await?;
        }

        Ok(())
    }

    pub async fn send_premium_welcome(&self, user_id: i32, plan_type: &str) -> Result<(), String> {
        let title = format!("Welcome to {}!", plan_type);
        let body = "You now have access to all premium features. Start connecting!";

        self.push.send(user_id, &title, body, None).await?;

        if let Some(ref in_app) = self.in_app {
            in_app
                .create(
                    user_id,
                    &title,
                    body,
                    "premium",
                    Some(&serde_json::json!({ "plan": plan_type })),
                )
                .await?;
        }

        Ok(())
    }

    // ============================================
    // Match Event Handlers
    // ============================================

    pub async fn send_match_notification(
        &self,
        user_id: i32,
        matched_with: i32,
    ) -> Result<(), String> {
        let data = serde_json::json!({
            "type": "new_match",
            "matched_user_id": matched_with,
        });

        // Use policy-gated send with bandit variant selection.
        // UTC offset 0 as default; in production, resolve from user's profile/device.
        let utc_offset = self.user_utc_offset(user_id).await;
        self.gated_send(user_id, NotifCategory::NewMatch, utc_offset, Some(&data)).await?;

        Ok(())
    }

    pub async fn send_like_notification(&self, user_id: i32, is_premium: bool) -> Result<(), String> {
        let data = serde_json::json!({
            "type": "like",
            "is_premium": is_premium,
        });

        let utc_offset = self.user_utc_offset(user_id).await;
        self.gated_send(user_id, NotifCategory::Like, utc_offset, Some(&data)).await?;
        Ok(())
    }

    pub async fn send_super_like_notification(
        &self,
        user_id: i32,
        from_user_id: i32,
    ) -> Result<(), String> {
        let data = serde_json::json!({
            "type": "super_like",
            "from_user_id": from_user_id,
        });

        self.push
            .send(
                user_id,
                "You Got a Super Like!",
                "Someone really likes you. Check it out!",
                Some(&data),
            )
            .await
    }

    // ============================================
    // Chat Event Handlers
    // ============================================

    pub async fn send_message_notification(
        &self,
        recipient_id: i32,
        sender_id: i32,
        content_preview: Option<&str>,
    ) -> Result<(), String> {
        let data = serde_json::json!({
            "type": "new_message",
            "sender_id": sender_id,
            "preview": content_preview.unwrap_or(""),
        });

        let utc_offset = self.user_utc_offset(recipient_id).await;
        self.gated_send(recipient_id, NotifCategory::Message, utc_offset, Some(&data)).await?;
        Ok(())
    }

    pub async fn send_read_receipt_notification(
        &self,
        user_id: i32,
        reader_id: i32,
    ) -> Result<(), String> {
        // This is a silent notification - no alert
        let data = serde_json::json!({
            "type": "message_read",
            "reader_id": reader_id,
        });

        // For iOS, we'd send a silent push with content-available
        // For now, we'll skip the notification for read receipts
        info!(
            user_id,
            reader_id, "Read receipt notification (silent)"
        );

        Ok(())
    }

    // ============================================
    // Payment Event Handlers
    // ============================================

    pub async fn send_payment_success(
        &self,
        user_id: i32,
        order_id: &str,
        amount_cents: i64,
    ) -> Result<(), String> {
        let amount = amount_cents as f64 / 100.0;
        let body = format!("Your payment of Rs.{:.2} was successful.", amount);

        self.push
            .send(user_id, "Payment Successful", &body, None)
            .await?;

        if let Some(ref in_app) = self.in_app {
            in_app
                .create(
                    user_id,
                    "Payment Successful",
                    &body,
                    "payment",
                    Some(&serde_json::json!({ "order_id": order_id, "amount": amount })),
                )
                .await?;
        }

        Ok(())
    }

    pub async fn send_payment_failed(&self, user_id: i32, error: &str) -> Result<(), String> {
        self.push
            .send(
                user_id,
                "Payment Failed",
                "Your payment could not be processed. Please try again.",
                None,
            )
            .await
    }

    pub async fn send_subscription_activated(
        &self,
        user_id: i32,
        plan_type: &str,
    ) -> Result<(), String> {
        let body = format!(
            "Your {} subscription is now active. Enjoy all premium features!",
            plan_type
        );

        self.push
            .send(user_id, "Subscription Activated", &body, None)
            .await?;

        if let Some(ref in_app) = self.in_app {
            in_app
                .create(
                    user_id,
                    "Subscription Activated",
                    &body,
                    "subscription",
                    Some(&serde_json::json!({ "plan": plan_type })),
                )
                .await?;
        }

        Ok(())
    }

    pub async fn send_subscription_cancelled(&self, user_id: i32) -> Result<(), String> {
        self.push
            .send(
                user_id,
                "Subscription Cancelled",
                "Your subscription has been cancelled. You can resubscribe anytime.",
                None,
            )
            .await
    }

    pub async fn send_subscription_expiring(
        &self,
        user_id: i32,
        days_left: i32,
    ) -> Result<(), String> {
        let body = format!(
            "Your subscription expires in {} days. Renew now to keep your premium features.",
            days_left
        );

        self.push
            .send(user_id, "Subscription Expiring Soon", &body, None)
            .await
    }

    // ============================================
    // Ambassador Event Handlers
    // ============================================

    pub async fn send_referral_signup(&self, ambassador_id: i32, referral_name: &str) -> Result<(), String> {
        let body = format!("{} signed up using your referral code!", referral_name);

        self.push
            .send(ambassador_id, "New Referral!", &body, None)
            .await
    }

    pub async fn send_commission_earned(
        &self,
        ambassador_id: i32,
        amount_cents: i64,
    ) -> Result<(), String> {
        let amount = amount_cents as f64 / 100.0;
        let body = format!("You earned Rs.{:.2} in commission!", amount);

        self.push
            .send(ambassador_id, "Commission Earned", &body, None)
            .await
    }

    // ============================================
    // Direct Notification Commands
    // ============================================

    pub async fn send_push(
        &self,
        user_id: i32,
        title: &str,
        body: &str,
        data: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        self.push.send(user_id, title, body, data).await
    }

    pub async fn send_email(
        &self,
        user_id: i32,
        template: &str,
        subject: &str,
        variables: &serde_json::Value,
    ) -> Result<(), String> {
        self.email.send(user_id, template, subject, variables).await
    }

    pub async fn send_sms(&self, phone_number: &str, message: &str) -> Result<(), String> {
        self.sms.send(phone_number, message).await
    }

    pub async fn send_otp(&self, phone_number: &str, otp: &str) -> Result<(), String> {
        self.sms.send_otp(phone_number, otp).await
    }

    // ============================================
    // Engagement Tracking
    // ============================================

    /// Record that a user opened/tapped a notification.
    /// Called from push-receipt webhook or analytics pipeline.
    pub async fn record_notification_engagement(
        &self,
        user_id: i32,
        variant_id: &str,
        engaged: bool,
    ) {
        self.policy.record_engagement(user_id, variant_id, engaged).await;
    }

    /// Get bandit statistics for monitoring dashboard.
    pub fn notification_stats(&self) -> serde_json::Value {
        self.policy.bandit_stats()
    }

    // ============================================
    // Private Helpers
    // ============================================

    /// Look up user's UTC offset. Resolution order:
    /// 1. Device-reported offset (most accurate, from push registration)
    /// 2. Country code from user profile → approximate offset
    /// 3. UTC (0) as global fallback
    async fn user_utc_offset(&self, user_id: i32) -> i32 {
        let Some(ref pool) = self.pool else {
            return 0;
        };

        // 1. Try device-reported offset
        let device_offset: Option<i32> = sqlx::query_scalar(
            r#"SELECT utc_offset_hours FROM user_devices
               WHERE user_id = $1 AND is_active = TRUE
               ORDER BY last_used_at DESC NULLS LAST
               LIMIT 1"#,
        )
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();

        if let Some(offset) = device_offset {
            return offset;
        }

        // 2. Fall back to country code from user profile
        let country: Option<String> = sqlx::query_scalar(
            "SELECT country_code FROM users WHERE id = $1",
        )
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();

        if let Some(cc) = country {
            return country_to_utc_offset(&cc);
        }

        0 // UTC fallback
    }
}

/// Map ISO 3166-1 alpha-2 country code to approximate UTC offset.
/// Uses the most populous timezone for countries with multiple zones.
fn country_to_utc_offset(country_code: &str) -> i32 {
    match country_code.to_uppercase().as_str() {
        // South Asia
        "IN" => 5,  // IST +5:30, rounded
        "LK" => 5,  // Sri Lanka
        "NP" => 5,  // Nepal +5:45
        "BD" => 6,  // Bangladesh
        "PK" => 5,  // Pakistan
        // Southeast Asia
        "SG" | "MY" | "PH" => 8,
        "TH" | "VN" | "ID" => 7,
        // East Asia
        "JP" => 9,
        "KR" => 9,
        "CN" | "TW" | "HK" => 8,
        // Middle East
        "AE" | "OM" => 4,
        "SA" | "QA" | "BH" | "KW" => 3,
        // Africa
        "NG" | "GH" => 1,
        "KE" | "ET" | "TZ" => 3,
        "ZA" | "EG" => 2,
        // Europe
        "GB" | "IE" | "PT" => 0,
        "FR" | "DE" | "ES" | "IT" | "NL" | "BE" | "AT" | "CH" | "PL" | "SE" | "NO" | "DK" => 1,
        "FI" | "GR" | "RO" | "BG" | "UA" => 2,
        "RU" => 3,  // Moscow time
        "TR" => 3,
        // Americas
        "US" => -5, // EST (most populous)
        "CA" => -5,
        "MX" => -6,
        "BR" => -3,
        "AR" => -3,
        "CL" => -4,
        "CO" | "PE" => -5,
        // Oceania
        "AU" => 10, // AEST
        "NZ" => 12,
        _ => 0,
    }
}
