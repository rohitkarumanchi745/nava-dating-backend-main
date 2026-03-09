//! Notification module — ported from microservices/notification-service.
//!
//! Provides push notifications (FCM/APNs), email (SMTP), SMS (Twilio),
//! in-app notifications, policy gating (caps, quiet hours, send-time),
//! and Thompson Sampling variant selection.
//!
//! Subscribes to the in-process `EventBus` instead of Kafka.

pub mod policy;
pub mod providers;
pub mod push;

use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::broadcast;
use tracing::{error, info, warn};

use crate::modules::events::{DomainEvent, EventEnvelope};

use self::policy::{NotifCategory, NotificationPolicy};
use self::providers::{EmailProvider, InAppProvider, PushProvider, SmsProvider};

// ─── NotificationModule ──────────────────────────────────────────────────────

/// Central notification orchestrator that listens to domain events
/// and dispatches notifications through the policy gate.
pub struct NotificationModule {
    pub handlers: Arc<NotificationHandlers>,
}

impl NotificationModule {
    pub async fn new(pool: PgPool) -> Self {
        // Ensure policy tables exist
        policy::ensure_policy_tables(&pool).await;

        let notif_policy = Arc::new(NotificationPolicy::new(pool.clone()).await);
        let handlers = Arc::new(NotificationHandlers::new(pool, notif_policy.clone()).await);

        // Periodically refresh send-time histograms (every 6 hours)
        let policy_refresh = notif_policy.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(6 * 3600));
            interval.tick().await; // skip first tick
            loop {
                interval.tick().await;
                info!("Refreshing notification send-time histograms");
                policy_refresh.refresh_send_time().await;
            }
        });

        Self { handlers }
    }

    /// Start listening to the event bus and dispatching notifications.
    pub fn start_listener(self: &Arc<Self>, mut rx: broadcast::Receiver<EventEnvelope>) {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(envelope) => {
                        let handlers = this.handlers.clone();
                        // Spawn so we don't block the event loop
                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_event(&handlers, envelope).await {
                                error!(error = %e, "Notification handler failed");
                            }
                        });
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "Notification listener lagged behind event bus");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("Event bus closed, notification listener shutting down");
                        break;
                    }
                }
            }
        });
    }

    async fn handle_event(
        handlers: &NotificationHandlers,
        envelope: EventEnvelope,
    ) -> Result<(), String> {
        match envelope.data {
            DomainEvent::UserRegistered { user_id } => {
                handlers.send_welcome_notification(user_id).await
            }
            DomainEvent::UserVerified { user_id, verification_type } => {
                handlers.send_verification_success(user_id, &verification_type).await
            }
            DomainEvent::PremiumActivated { user_id, plan_type } => {
                handlers.send_premium_welcome(user_id, &plan_type).await
            }
            DomainEvent::MatchCreated { user1_id, user2_id, .. } => {
                handlers.send_match_notification(user1_id, user2_id).await?;
                handlers.send_match_notification(user2_id, user1_id).await
            }
            DomainEvent::MessageSent { recipient_id, sender_id, content_preview, .. } => {
                handlers.send_message_notification(recipient_id, sender_id, content_preview.as_deref()).await
            }
            DomainEvent::PaymentCompleted { order_id, user_id, amount_cents, .. } => {
                handlers.send_payment_success(user_id, &order_id, amount_cents).await
            }
            DomainEvent::PaymentFailed { user_id, error_message, .. } => {
                handlers.send_payment_failed(user_id, &error_message).await
            }
            DomainEvent::SubscriptionActivated { user_id, plan_type, .. } => {
                handlers.send_subscription_activated(user_id, &plan_type).await
            }
            DomainEvent::SubscriptionCancelled { user_id, .. } => {
                handlers.send_subscription_cancelled(user_id).await
            }
            DomainEvent::ReferralSignup { ambassador_id, referral_name, .. } => {
                handlers.send_referral_signup(ambassador_id, &referral_name).await
            }
            DomainEvent::CommissionEarned { ambassador_id, amount_cents } => {
                handlers.send_commission_earned(ambassador_id, amount_cents).await
            }
            DomainEvent::SendPush { user_id, title, body, data } => {
                handlers.send_push(user_id, &title, &body, data.as_ref()).await
            }
            DomainEvent::SendEmail { user_id, template, subject, variables } => {
                handlers.send_email(user_id, &template, &subject, &variables).await
            }
            DomainEvent::SendSms { phone_number, message } => {
                handlers.send_sms(&phone_number, &message).await
            }
            // Events we don't need to send notifications for
            _ => Ok(()),
        }
    }
}

// ─── NotificationHandlers ────────────────────────────────────────────────────

pub struct NotificationHandlers {
    pool: PgPool,
    push: PushProvider,
    email: EmailProvider,
    sms: SmsProvider,
    in_app: Option<InAppProvider>,
    policy: Arc<NotificationPolicy>,
}

impl NotificationHandlers {
    pub async fn new(pool: PgPool, policy: Arc<NotificationPolicy>) -> Self {
        let push = PushProvider::new(pool.clone()).await;

        let in_app_provider = InAppProvider::new(pool.clone());
        if let Err(e) = in_app_provider.ensure_table().await {
            error!(error = %e, "Failed to ensure in_app_notifications table");
        }

        Self {
            pool: pool.clone(),
            push,
            email: EmailProvider::new(),
            sms: SmsProvider::new(),
            in_app: Some(in_app_provider),
            policy,
        }
    }

    /// Run a notification through the policy gate.
    async fn gated_send(
        &self,
        user_id: i32,
        category: NotifCategory,
        utc_offset_hours: i32,
        data: Option<&serde_json::Value>,
    ) -> Result<bool, String> {
        use chrono::{Timelike, Utc};
        use policy::GateDecision;

        let decision = self.policy.gate(user_id, category, utc_offset_hours).await;

        match decision {
            GateDecision::Send { variant, bandit_selected } => {
                let sent_hour = ((Utc::now().hour() as i32 + utc_offset_hours).rem_euclid(24)) as u8;

                self.push.send(user_id, &variant.title, &variant.body, data).await?;

                if let Some(ref in_app) = self.in_app {
                    let _ = in_app.create(
                        user_id, &variant.title, &variant.body,
                        category.as_str(), data,
                    ).await;
                }

                self.policy.log_outcome(
                    user_id, category.as_str(), &variant.variant_id,
                    bandit_selected, sent_hour,
                ).await;

                Ok(true)
            }
            GateDecision::Defer { .. } | GateDecision::Suppress { .. } => Ok(false),
        }
    }

    // ─── User events ─────────────────────────────────────────────────

    pub async fn send_welcome_notification(&self, user_id: i32) -> Result<(), String> {
        self.push.send(user_id, "Welcome to Nava!", "Start your journey to find meaningful connections.", None).await?;
        if let Some(ref in_app) = self.in_app {
            in_app.create(user_id, "Welcome to Nava!", "Complete your profile to start matching with people.", "welcome", None).await?;
        }
        Ok(())
    }

    pub async fn send_verification_success(&self, user_id: i32, verification_type: &str) -> Result<(), String> {
        let (title, message) = match verification_type {
            "phone" => ("Phone Verified", "Your phone number has been verified!"),
            "email" => ("Email Verified", "Your email has been verified!"),
            "selfie" => ("Selfie Verified", "Your selfie verification is complete. Your profile is now verified!"),
            "student" => ("Student Verified", "Your student status has been verified!"),
            _ => ("Verification Complete", "Your account has been verified!"),
        };
        self.push.send(user_id, title, message, None).await?;
        if let Some(ref in_app) = self.in_app {
            in_app.create(user_id, title, message, "verification", None).await?;
        }
        Ok(())
    }

    pub async fn send_premium_welcome(&self, user_id: i32, plan_type: &str) -> Result<(), String> {
        let title = format!("Welcome to {}!", plan_type);
        let body = "You now have access to all premium features. Start connecting!";
        self.push.send(user_id, &title, body, None).await?;
        if let Some(ref in_app) = self.in_app {
            in_app.create(user_id, &title, body, "premium", Some(&serde_json::json!({ "plan": plan_type }))).await?;
        }
        Ok(())
    }

    // ─── Match events ────────────────────────────────────────────────

    pub async fn send_match_notification(&self, user_id: i32, matched_with: i32) -> Result<(), String> {
        let data = serde_json::json!({ "type": "new_match", "matched_user_id": matched_with });
        let utc_offset = self.user_utc_offset(user_id).await;
        self.gated_send(user_id, NotifCategory::NewMatch, utc_offset, Some(&data)).await?;
        Ok(())
    }

    // ─── Chat events ─────────────────────────────────────────────────

    pub async fn send_message_notification(&self, recipient_id: i32, sender_id: i32, content_preview: Option<&str>) -> Result<(), String> {
        let data = serde_json::json!({ "type": "new_message", "sender_id": sender_id, "preview": content_preview.unwrap_or("") });
        let utc_offset = self.user_utc_offset(recipient_id).await;
        self.gated_send(recipient_id, NotifCategory::Message, utc_offset, Some(&data)).await?;
        Ok(())
    }

    // ─── Payment events ──────────────────────────────────────────────

    pub async fn send_payment_success(&self, user_id: i32, order_id: &str, amount_cents: i64) -> Result<(), String> {
        let amount = amount_cents as f64 / 100.0;
        let body = format!("Your payment of Rs.{:.2} was successful.", amount);
        self.push.send(user_id, "Payment Successful", &body, None).await?;
        if let Some(ref in_app) = self.in_app {
            in_app.create(user_id, "Payment Successful", &body, "payment", Some(&serde_json::json!({ "order_id": order_id, "amount": amount }))).await?;
        }
        Ok(())
    }

    pub async fn send_payment_failed(&self, user_id: i32, _error: &str) -> Result<(), String> {
        self.push.send(user_id, "Payment Failed", "Your payment could not be processed. Please try again.", None).await
    }

    pub async fn send_subscription_activated(&self, user_id: i32, plan_type: &str) -> Result<(), String> {
        let body = format!("Your {} subscription is now active. Enjoy all premium features!", plan_type);
        self.push.send(user_id, "Subscription Activated", &body, None).await?;
        if let Some(ref in_app) = self.in_app {
            in_app.create(user_id, "Subscription Activated", &body, "subscription", Some(&serde_json::json!({ "plan": plan_type }))).await?;
        }
        Ok(())
    }

    pub async fn send_subscription_cancelled(&self, user_id: i32) -> Result<(), String> {
        self.push.send(user_id, "Subscription Cancelled", "Your subscription has been cancelled. You can resubscribe anytime.", None).await
    }

    // ─── Ambassador events ───────────────────────────────────────────

    pub async fn send_referral_signup(&self, ambassador_id: i32, referral_name: &str) -> Result<(), String> {
        let body = format!("{} signed up using your referral code!", referral_name);
        self.push.send(ambassador_id, "New Referral!", &body, None).await
    }

    pub async fn send_commission_earned(&self, ambassador_id: i32, amount_cents: i64) -> Result<(), String> {
        let amount = amount_cents as f64 / 100.0;
        let body = format!("You earned Rs.{:.2} in commission!", amount);
        self.push.send(ambassador_id, "Commission Earned", &body, None).await
    }

    // ─── Direct commands ─────────────────────────────────────────────

    pub async fn send_push(&self, user_id: i32, title: &str, body: &str, data: Option<&serde_json::Value>) -> Result<(), String> {
        self.push.send(user_id, title, body, data).await
    }

    pub async fn send_email(&self, user_id: i32, template: &str, subject: &str, variables: &serde_json::Value) -> Result<(), String> {
        self.email.send(user_id, template, subject, variables).await
    }

    pub async fn send_sms(&self, phone_number: &str, message: &str) -> Result<(), String> {
        self.sms.send(phone_number, message).await
    }

    // ─── Engagement tracking ─────────────────────────────────────────

    pub async fn record_notification_engagement(&self, user_id: i32, variant_id: &str, engaged: bool) {
        self.policy.record_engagement(user_id, variant_id, engaged).await;
    }

    pub fn notification_stats(&self) -> serde_json::Value {
        self.policy.bandit_stats()
    }

    pub fn policy_metrics_prometheus(&self) -> String {
        self.policy.metrics_prometheus()
    }

    // ─── Private ─────────────────────────────────────────────────────

    async fn user_utc_offset(&self, user_id: i32) -> i32 {
        let device_offset: Option<i32> = sqlx::query_scalar(
            r#"SELECT utc_offset_hours FROM user_devices
               WHERE user_id = $1 AND is_active = TRUE
               ORDER BY last_used_at DESC NULLS LAST LIMIT 1"#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        if let Some(offset) = device_offset {
            return offset;
        }

        let country: Option<String> = sqlx::query_scalar(
            "SELECT country_code FROM users WHERE id = $1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        if let Some(cc) = country {
            return country_to_utc_offset(&cc);
        }

        0
    }
}

fn country_to_utc_offset(country_code: &str) -> i32 {
    match country_code.to_uppercase().as_str() {
        "IN" => 5, "LK" => 5, "NP" => 5, "BD" => 6, "PK" => 5,
        "SG" | "MY" | "PH" => 8, "TH" | "VN" | "ID" => 7,
        "JP" => 9, "KR" => 9, "CN" | "TW" | "HK" => 8,
        "AE" | "OM" => 4, "SA" | "QA" | "BH" | "KW" => 3,
        "NG" | "GH" => 1, "KE" | "ET" | "TZ" => 3, "ZA" | "EG" => 2,
        "GB" | "IE" | "PT" => 0,
        "FR" | "DE" | "ES" | "IT" | "NL" | "BE" | "AT" | "CH" | "PL" | "SE" | "NO" | "DK" => 1,
        "FI" | "GR" | "RO" | "BG" | "UA" => 2,
        "RU" => 3, "TR" => 3,
        "US" => -5, "CA" => -5, "MX" => -6, "BR" => -3, "AR" => -3,
        "CL" => -4, "CO" | "PE" => -5,
        "AU" => 10, "NZ" => 12,
        _ => 0,
    }
}
