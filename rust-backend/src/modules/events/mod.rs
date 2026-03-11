//! In-process event bus — replaces Kafka for modular monolith.
//!
//! Uses `tokio::sync::broadcast` for fan-out to multiple subscribers.
//! Each domain module can publish and subscribe to typed events without
//! network hops or serialization overhead.

use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::warn;

// ─── Event Envelope ──────────────────────────────────────────────────────────

/// Wrapper around every domain event with tracing metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub event_id: String,
    pub event_type: String,
    pub timestamp: DateTime<Utc>,
    pub source: &'static str,
    pub data: DomainEvent,
}

// ─── Domain Events ───────────────────────────────────────────────────────────

/// All domain events that can flow through the bus.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum DomainEvent {
    // User lifecycle
    UserRegistered { user_id: i32 },
    UserVerified { user_id: i32, verification_type: String },
    PremiumActivated { user_id: i32, plan_type: String },

    // Matching
    MatchCreated { match_id: i32, user1_id: i32, user2_id: i32 },
    SwipeLike { user_id: i32, target_user_id: i32 },
    SwipeSuperLike { user_id: i32, target_user_id: i32 },
    SwipePass { user_id: i32, target_user_id: i32 },

    // Chat
    MessageSent {
        message_id: i32,
        match_id: i32,
        sender_id: i32,
        recipient_id: i32,
        content_preview: Option<String>,
    },

    // Payments
    OrderCreated { order_id: String, user_id: i32, amount_cents: i64, currency: String },
    PaymentCompleted { order_id: String, user_id: i32, transaction_id: String, amount_cents: i64 },
    PaymentFailed { order_id: String, user_id: i32, error_message: String },
    SubscriptionActivated { subscription_id: String, user_id: i32, plan_type: String },
    SubscriptionCancelled { subscription_id: String, user_id: i32 },

    // Ambassador
    ReferralSignup { ambassador_id: i32, referred_user_id: i32, referral_name: String },
    CommissionEarned { ambassador_id: i32, amount_cents: i64 },

    // Reels
    ReelMessage { reel_id: i32, sender_id: i32, receiver_id: i32, content_preview: String },
    ReelReply { reel_id: i32, replier_id: i32, original_sender_id: i32, content_preview: String },
    ReelMatchEligible { reel_id: i32, user_a: i32, user_b: i32 },
    ReelMatchRequested { reel_id: i32, requester_id: i32, target_id: i32 },
    ReelMatchAccepted { reel_id: i32, match_id: String, user1_id: i32, user2_id: i32 },

    // Notifications (commands)
    SendPush { user_id: i32, title: String, body: String, data: Option<serde_json::Value> },
    SendEmail { user_id: i32, template: String, subject: String, variables: serde_json::Value },
    SendSms { phone_number: String, message: String },

    // Analytics
    AnalyticsEvent { event_type: String, user_id: Option<i32>, data: serde_json::Value },
}

// ─── Event Bus ───────────────────────────────────────────────────────────────

/// Metrics for the event bus.
pub struct EventBusMetrics {
    pub events_published: AtomicU64,
    pub events_dropped: AtomicU64,
}

impl EventBusMetrics {
    fn new() -> Self {
        Self {
            events_published: AtomicU64::new(0),
            events_dropped: AtomicU64::new(0),
        }
    }
}

/// In-process event bus backed by `tokio::sync::broadcast`.
pub struct EventBus {
    tx: broadcast::Sender<EventEnvelope>,
    pub metrics: EventBusMetrics,
}

impl EventBus {
    /// Create a new event bus with the given channel capacity.
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self {
            tx,
            metrics: EventBusMetrics::new(),
        }
    }

    /// Publish a domain event. Non-blocking; drops if all receivers are behind.
    pub fn publish(&self, source: &'static str, event: DomainEvent) {
        let event_type = match &event {
            DomainEvent::UserRegistered { .. } => "user.registered",
            DomainEvent::UserVerified { .. } => "user.verified",
            DomainEvent::PremiumActivated { .. } => "user.premium_activated",
            DomainEvent::MatchCreated { .. } => "match.created",
            DomainEvent::SwipeLike { .. } => "match.swipe_like",
            DomainEvent::SwipeSuperLike { .. } => "match.swipe_super_like",
            DomainEvent::SwipePass { .. } => "match.swipe_pass",
            DomainEvent::MessageSent { .. } => "chat.message_sent",
            DomainEvent::OrderCreated { .. } => "payment.order_created",
            DomainEvent::PaymentCompleted { .. } => "payment.completed",
            DomainEvent::PaymentFailed { .. } => "payment.failed",
            DomainEvent::SubscriptionActivated { .. } => "payment.subscription_activated",
            DomainEvent::SubscriptionCancelled { .. } => "payment.subscription_cancelled",
            DomainEvent::ReferralSignup { .. } => "ambassador.referral_signup",
            DomainEvent::CommissionEarned { .. } => "ambassador.commission_earned",
            DomainEvent::ReelMessage { .. } => "reel.message",
            DomainEvent::ReelReply { .. } => "reel.reply",
            DomainEvent::ReelMatchEligible { .. } => "reel.match_eligible",
            DomainEvent::ReelMatchRequested { .. } => "reel.match_requested",
            DomainEvent::ReelMatchAccepted { .. } => "reel.match_accepted",
            DomainEvent::SendPush { .. } => "notification.send_push",
            DomainEvent::SendEmail { .. } => "notification.send_email",
            DomainEvent::SendSms { .. } => "notification.send_sms",
            DomainEvent::AnalyticsEvent { .. } => "analytics.event",
        };

        let envelope = EventEnvelope {
            event_id: uuid::Uuid::new_v4().to_string(),
            event_type: event_type.to_string(),
            timestamp: Utc::now(),
            source,
            data: event,
        };

        match self.tx.send(envelope) {
            Ok(_) => {
                self.metrics.events_published.fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => {
                self.metrics.events_dropped.fetch_add(1, Ordering::Relaxed);
                warn!(event_type, source, "Event dropped (no active subscribers)");
            }
        }
    }

    /// Subscribe to all events on the bus.
    pub fn subscribe(&self) -> broadcast::Receiver<EventEnvelope> {
        self.tx.subscribe()
    }

    /// Current number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(4096)
    }
}
