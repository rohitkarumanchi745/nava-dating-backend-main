//! Event type definitions for Kafka messages
//!
//! All events follow a common envelope structure with metadata for
//! tracing, idempotency, and versioning.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// =============================================================================
// Event Envelope
// =============================================================================

/// Common envelope for all events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope<T> {
    /// Unique event identifier
    pub event_id: Uuid,
    /// Event type string (e.g., "user.registered")
    pub event_type: String,
    /// ISO8601 timestamp
    pub timestamp: DateTime<Utc>,
    /// Event payload
    pub data: T,
    /// Event metadata
    pub metadata: EventMetadata,
}

impl<T: Serialize> EventEnvelope<T> {
    pub fn new(event_type: impl Into<String>, data: T, source_service: impl Into<String>) -> Self {
        Self {
            event_id: Uuid::new_v4(),
            event_type: event_type.into(),
            timestamp: Utc::now(),
            data,
            metadata: EventMetadata {
                source_service: source_service.into(),
                correlation_id: Uuid::new_v4(),
                causation_id: None,
                version: 1,
                idempotency_key: None,
            },
        }
    }

    pub fn with_correlation_id(mut self, correlation_id: Uuid) -> Self {
        self.metadata.correlation_id = correlation_id;
        self
    }

    pub fn with_causation_id(mut self, causation_id: Uuid) -> Self {
        self.metadata.causation_id = Some(causation_id);
        self
    }

    pub fn with_idempotency_key(mut self, key: String) -> Self {
        self.metadata.idempotency_key = Some(key);
        self
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

/// Event metadata for tracing and idempotency
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMetadata {
    /// Service that produced the event
    pub source_service: String,
    /// Correlation ID for distributed tracing
    pub correlation_id: Uuid,
    /// ID of the event that caused this event
    pub causation_id: Option<Uuid>,
    /// Event schema version
    pub version: u32,
    /// Idempotency key for deduplication
    pub idempotency_key: Option<String>,
}

// =============================================================================
// Kafka Topics
// =============================================================================

/// Kafka topic names
pub mod topics {
    pub const USER_EVENTS: &str = "user.events";
    pub const PAYMENT_EVENTS: &str = "payment.events";
    pub const MATCH_EVENTS: &str = "match.events";
    pub const CHAT_EVENTS: &str = "chat.events";
    pub const NOTIFICATION_COMMANDS: &str = "notification.commands";
    pub const ANALYTICS_EVENTS: &str = "analytics.events";
    pub const DLQ_EVENTS: &str = "dlq.events";
}

// =============================================================================
// User Events
// =============================================================================

/// User event types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum UserEvent {
    #[serde(rename = "user.registered")]
    Registered(UserRegisteredData),
    #[serde(rename = "user.verified")]
    Verified(UserVerifiedData),
    #[serde(rename = "user.profile_updated")]
    ProfileUpdated(UserProfileUpdatedData),
    #[serde(rename = "user.premium_activated")]
    PremiumActivated(UserPremiumActivatedData),
    #[serde(rename = "user.deleted")]
    Deleted(UserDeletedData),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRegisteredData {
    pub user_id: i32,
    pub phone_number: String,
    pub registration_source: String,
    pub referral_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserVerifiedData {
    pub user_id: i32,
    pub verification_type: String, // "phone", "email", "student", "selfie"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfileUpdatedData {
    pub user_id: i32,
    pub updated_fields: Vec<String>,
    pub is_complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPremiumActivatedData {
    pub user_id: i32,
    pub plan_type: String,
    pub expires_at: DateTime<Utc>,
    pub source: String, // "payment", "referral", "promo"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDeletedData {
    pub user_id: i32,
    pub reason: Option<String>,
}

// =============================================================================
// Payment Events
// =============================================================================

/// Payment event types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum PaymentEvent {
    #[serde(rename = "payment.order_created")]
    OrderCreated(OrderCreatedData),
    #[serde(rename = "payment.completed")]
    Completed(PaymentCompletedData),
    #[serde(rename = "payment.failed")]
    Failed(PaymentFailedData),
    #[serde(rename = "payment.subscription_activated")]
    SubscriptionActivated(SubscriptionActivatedData),
    #[serde(rename = "payment.subscription_cancelled")]
    SubscriptionCancelled(SubscriptionCancelledData),
    #[serde(rename = "payment.refund_processed")]
    RefundProcessed(RefundProcessedData),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderCreatedData {
    pub order_id: String,
    pub user_id: i32,
    pub product_id: String,
    pub amount_cents: i64,
    pub currency: String,
    pub gateway: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentCompletedData {
    pub order_id: String,
    pub user_id: i32,
    pub transaction_id: String,
    pub amount_cents: i64,
    pub currency: String,
    pub gateway: String,
    pub payment_method: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentFailedData {
    pub order_id: String,
    pub user_id: i32,
    pub error_code: String,
    pub error_message: String,
    pub gateway: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionActivatedData {
    pub subscription_id: String,
    pub user_id: i32,
    pub plan_type: String,
    pub starts_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub gateway: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionCancelledData {
    pub subscription_id: String,
    pub user_id: i32,
    pub reason: Option<String>,
    pub effective_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefundProcessedData {
    pub refund_id: String,
    pub order_id: String,
    pub user_id: i32,
    pub amount_cents: i64,
    pub reason: String,
}

// =============================================================================
// Match Events
// =============================================================================

/// Match event types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum MatchEvent {
    #[serde(rename = "match.swipe_like")]
    SwipeLike(SwipeData),
    #[serde(rename = "match.swipe_pass")]
    SwipePass(SwipeData),
    #[serde(rename = "match.created")]
    Created(MatchCreatedData),
    #[serde(rename = "match.unmatched")]
    Unmatched(MatchUnmatchedData),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwipeData {
    pub user_id: i32,
    pub target_user_id: i32,
    pub source: String, // "discover", "nearby", "university"
    pub score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchCreatedData {
    pub match_id: String,
    pub user_id_1: i32,
    pub user_id_2: i32,
    pub compatibility_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchUnmatchedData {
    pub match_id: String,
    pub initiated_by: i32,
    pub reason: Option<String>,
}

// =============================================================================
// Chat Events
// =============================================================================

/// Chat event types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ChatEvent {
    #[serde(rename = "chat.message_sent")]
    MessageSent(MessageSentData),
    #[serde(rename = "chat.message_read")]
    MessageRead(MessageReadData),
    #[serde(rename = "chat.conversation_started")]
    ConversationStarted(ConversationStartedData),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSentData {
    pub message_id: String,
    pub match_id: String,
    pub sender_id: i32,
    pub recipient_id: i32,
    pub message_type: String, // "text", "image", "voice"
    pub content_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageReadData {
    pub message_id: String,
    pub match_id: String,
    pub reader_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationStartedData {
    pub match_id: String,
    pub user_id_1: i32,
    pub user_id_2: i32,
}

// =============================================================================
// Notification Commands
// =============================================================================

/// Notification command types (commands, not events)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum NotificationCommand {
    #[serde(rename = "notification.send_push")]
    SendPush(SendPushData),
    #[serde(rename = "notification.send_email")]
    SendEmail(SendEmailData),
    #[serde(rename = "notification.send_sms")]
    SendSms(SendSmsData),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendPushData {
    pub user_id: i32,
    pub title: String,
    pub body: String,
    pub data: Option<serde_json::Value>,
    pub badge_count: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendEmailData {
    pub user_id: i32,
    pub template: String,
    pub subject: String,
    pub variables: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendSmsData {
    pub phone_number: String,
    pub message: String,
}

// =============================================================================
// Analytics Events
// =============================================================================

/// Analytics event for tracking user behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsEvent {
    pub event_name: String,
    pub user_id: Option<i32>,
    pub session_id: Option<String>,
    pub properties: serde_json::Value,
    pub context: AnalyticsContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsContext {
    pub app_version: Option<String>,
    pub platform: Option<String>,
    pub device_type: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

// =============================================================================
// Dead Letter Queue Event
// =============================================================================

/// DLQ entry for failed event processing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DlqEvent {
    pub original_topic: String,
    pub original_partition: i32,
    pub original_offset: i64,
    pub original_payload: String,
    pub error_message: String,
    pub retry_count: i32,
    pub first_failed_at: DateTime<Utc>,
    pub last_failed_at: DateTime<Utc>,
}

// =============================================================================
// Event Type Helpers
// =============================================================================

impl UserEvent {
    pub fn event_type(&self) -> &'static str {
        match self {
            UserEvent::Registered(_) => "user.registered",
            UserEvent::Verified(_) => "user.verified",
            UserEvent::ProfileUpdated(_) => "user.profile_updated",
            UserEvent::PremiumActivated(_) => "user.premium_activated",
            UserEvent::Deleted(_) => "user.deleted",
        }
    }

    pub fn user_id(&self) -> i32 {
        match self {
            UserEvent::Registered(d) => d.user_id,
            UserEvent::Verified(d) => d.user_id,
            UserEvent::ProfileUpdated(d) => d.user_id,
            UserEvent::PremiumActivated(d) => d.user_id,
            UserEvent::Deleted(d) => d.user_id,
        }
    }
}

impl PaymentEvent {
    pub fn event_type(&self) -> &'static str {
        match self {
            PaymentEvent::OrderCreated(_) => "payment.order_created",
            PaymentEvent::Completed(_) => "payment.completed",
            PaymentEvent::Failed(_) => "payment.failed",
            PaymentEvent::SubscriptionActivated(_) => "payment.subscription_activated",
            PaymentEvent::SubscriptionCancelled(_) => "payment.subscription_cancelled",
            PaymentEvent::RefundProcessed(_) => "payment.refund_processed",
        }
    }

    pub fn user_id(&self) -> i32 {
        match self {
            PaymentEvent::OrderCreated(d) => d.user_id,
            PaymentEvent::Completed(d) => d.user_id,
            PaymentEvent::Failed(d) => d.user_id,
            PaymentEvent::SubscriptionActivated(d) => d.user_id,
            PaymentEvent::SubscriptionCancelled(d) => d.user_id,
            PaymentEvent::RefundProcessed(d) => d.user_id,
        }
    }
}

impl MatchEvent {
    pub fn event_type(&self) -> &'static str {
        match self {
            MatchEvent::SwipeLike(_) => "match.swipe_like",
            MatchEvent::SwipePass(_) => "match.swipe_pass",
            MatchEvent::Created(_) => "match.created",
            MatchEvent::Unmatched(_) => "match.unmatched",
        }
    }
}
