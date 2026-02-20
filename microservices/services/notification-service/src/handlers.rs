//! Notification handlers for different event types
//!
//! Contains business logic for sending notifications via different channels.

use sqlx::PgPool;
use tracing::{error, info, warn};

use crate::providers::{EmailProvider, InAppProvider, PushProvider, SmsProvider};

pub struct NotificationHandlers {
    push: PushProvider,
    email: EmailProvider,
    sms: SmsProvider,
    in_app: Option<InAppProvider>,
}

impl NotificationHandlers {
    pub async fn new(pool: Option<PgPool>) -> Self {
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
            push,
            email: EmailProvider::new(),
            sms: SmsProvider::new(),
            in_app,
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

        self.push
            .send(
                user_id,
                "It's a Match!",
                "You have a new match! Start a conversation now.",
                Some(&data),
            )
            .await?;

        if let Some(ref in_app) = self.in_app {
            in_app
                .create(
                    user_id,
                    "It's a Match!",
                    "You have a new match! Start a conversation now.",
                    "match",
                    Some(&data),
                )
                .await?;
        }

        Ok(())
    }

    pub async fn send_like_notification(&self, user_id: i32, is_premium: bool) -> Result<(), String> {
        if is_premium {
            self.push
                .send(
                    user_id,
                    "Someone Likes You!",
                    "Check out who likes you in the Likes tab.",
                    None,
                )
                .await
        } else {
            self.push
                .send(
                    user_id,
                    "Someone Likes You!",
                    "Upgrade to Premium to see who likes you.",
                    None,
                )
                .await
        }
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
        let body = content_preview.unwrap_or("You have a new message");

        let data = serde_json::json!({
            "type": "new_message",
            "sender_id": sender_id,
        });

        self.push
            .send(recipient_id, "New Message", body, Some(&data))
            .await
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
}
