//! Notification Service
//!
//! Event-driven notification service that consumes events from Kafka
//! and sends push notifications (FCM for Android, APNs for iOS), emails, and SMS.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{routing::get, Router};
use shared::events::{
    consumer::{ConsumerConfig, EventConsumer, EventHandler},
    types::{
        topics, ChatEvent, EventEnvelope, MatchEvent, NotificationCommand, PaymentEvent, UserEvent,
    },
};
use sqlx::PgPool;
use tokio::signal;
use tower_http::trace::TraceLayer;
use tracing::{error, info, Level};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod handlers;
mod providers;
pub mod push;

use handlers::NotificationHandlers;

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "notification_service=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    dotenvy::dotenv().ok();

    let port = std::env::var("NOTIFICATION_SERVICE_PORT")
        .unwrap_or_else(|_| "8007".to_string())
        .parse::<u16>()
        .expect("Invalid port");

    // Connect to database for device token management
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://localhost/nava".to_string());

    let pool = PgPool::connect(&database_url)
        .await
        .expect("Failed to connect to database");

    info!("Connected to database");

    // Initialize notification handlers with database pool
    let handlers = Arc::new(NotificationHandlers::new(Some(pool)).await);

    // Start Kafka consumers in background
    let handlers_clone = handlers.clone();
    tokio::spawn(async move {
        start_kafka_consumers(handlers_clone).await;
    });

    // Health check API
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/ready", get(readiness_check))
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .expect("Failed to bind");

    info!("Notification service listening on port {}", port);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("Server error");
}

async fn health_check() -> &'static str {
    "OK"
}

async fn readiness_check() -> &'static str {
    "READY"
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("Shutdown signal received");
}

async fn start_kafka_consumers(handlers: Arc<NotificationHandlers>) {
    let group_id = std::env::var("KAFKA_GROUP_ID")
        .unwrap_or_else(|_| "notification-consumer-group".to_string());

    // User events consumer
    let user_handlers = handlers.clone();
    let user_group = format!("{}-user", group_id);
    tokio::spawn(async move {
        consume_user_events(&user_group, user_handlers).await;
    });

    // Match events consumer
    let match_handlers = handlers.clone();
    let match_group = format!("{}-match", group_id);
    tokio::spawn(async move {
        consume_match_events(&match_group, match_handlers).await;
    });

    // Chat events consumer
    let chat_handlers = handlers.clone();
    let chat_group = format!("{}-chat", group_id);
    tokio::spawn(async move {
        consume_chat_events(&chat_group, chat_handlers).await;
    });

    // Payment events consumer
    let payment_handlers = handlers.clone();
    let payment_group = format!("{}-payment", group_id);
    tokio::spawn(async move {
        consume_payment_events(&payment_group, payment_handlers).await;
    });

    // Notification commands consumer
    let cmd_handlers = handlers.clone();
    let cmd_group = format!("{}-commands", group_id);
    tokio::spawn(async move {
        consume_notification_commands(&cmd_group, cmd_handlers).await;
    });

    info!("All Kafka consumers started");
}

async fn consume_user_events(group_id: &str, handlers: Arc<NotificationHandlers>) {
    let config = ConsumerConfig::from_env(group_id);

    match EventConsumer::new(config) {
        Ok(consumer) => {
            if let Err(e) = consumer.subscribe(&[topics::USER_EVENTS]) {
                error!("Failed to subscribe to user events: {}", e);
                return;
            }

            let handler = UserEventHandler { handlers };
            if let Err(e) = consumer.consume_with_retry(handler).await {
                error!("User events consumer error: {}", e);
            }
        }
        Err(e) => {
            error!("Failed to create user events consumer: {}", e);
        }
    }
}

async fn consume_match_events(group_id: &str, handlers: Arc<NotificationHandlers>) {
    let config = ConsumerConfig::from_env(group_id);

    match EventConsumer::new(config) {
        Ok(consumer) => {
            if let Err(e) = consumer.subscribe(&[topics::MATCH_EVENTS]) {
                error!("Failed to subscribe to match events: {}", e);
                return;
            }

            let handler = MatchEventHandler { handlers };
            if let Err(e) = consumer.consume_with_retry(handler).await {
                error!("Match events consumer error: {}", e);
            }
        }
        Err(e) => {
            error!("Failed to create match events consumer: {}", e);
        }
    }
}

async fn consume_chat_events(group_id: &str, handlers: Arc<NotificationHandlers>) {
    let config = ConsumerConfig::from_env(group_id);

    match EventConsumer::new(config) {
        Ok(consumer) => {
            if let Err(e) = consumer.subscribe(&[topics::CHAT_EVENTS]) {
                error!("Failed to subscribe to chat events: {}", e);
                return;
            }

            let handler = ChatEventHandler { handlers };
            if let Err(e) = consumer.consume_with_retry(handler).await {
                error!("Chat events consumer error: {}", e);
            }
        }
        Err(e) => {
            error!("Failed to create chat events consumer: {}", e);
        }
    }
}

async fn consume_payment_events(group_id: &str, handlers: Arc<NotificationHandlers>) {
    let config = ConsumerConfig::from_env(group_id);

    match EventConsumer::new(config) {
        Ok(consumer) => {
            if let Err(e) = consumer.subscribe(&[topics::PAYMENT_EVENTS]) {
                error!("Failed to subscribe to payment events: {}", e);
                return;
            }

            let handler = PaymentEventHandler { handlers };
            if let Err(e) = consumer.consume_with_retry(handler).await {
                error!("Payment events consumer error: {}", e);
            }
        }
        Err(e) => {
            error!("Failed to create payment events consumer: {}", e);
        }
    }
}

async fn consume_notification_commands(group_id: &str, handlers: Arc<NotificationHandlers>) {
    let config = ConsumerConfig::from_env(group_id);

    match EventConsumer::new(config) {
        Ok(consumer) => {
            if let Err(e) = consumer.subscribe(&[topics::NOTIFICATION_COMMANDS]) {
                error!("Failed to subscribe to notification commands: {}", e);
                return;
            }

            let handler = NotificationCommandHandler { handlers };
            if let Err(e) = consumer.consume_with_retry(handler).await {
                error!("Notification commands consumer error: {}", e);
            }
        }
        Err(e) => {
            error!("Failed to create notification commands consumer: {}", e);
        }
    }
}

// Event Handlers

struct UserEventHandler {
    handlers: Arc<NotificationHandlers>,
}

#[async_trait]
impl EventHandler<UserEvent> for UserEventHandler {
    async fn handle(&self, event: EventEnvelope<UserEvent>) -> Result<(), String> {
        match &event.data {
            UserEvent::Registered(data) => {
                info!(user_id = data.user_id, "New user registered - sending welcome notification");
                self.handlers
                    .send_welcome_notification(data.user_id)
                    .await
            }
            UserEvent::Verified(data) => {
                info!(user_id = data.user_id, verification_type = %data.verification_type, "User verified");
                self.handlers
                    .send_verification_success(data.user_id, &data.verification_type)
                    .await
            }
            UserEvent::PremiumActivated(data) => {
                info!(user_id = data.user_id, plan = %data.plan_type, "Premium activated");
                self.handlers
                    .send_premium_welcome(data.user_id, &data.plan_type)
                    .await
            }
            _ => Ok(()),
        }
    }
}

struct MatchEventHandler {
    handlers: Arc<NotificationHandlers>,
}

#[async_trait]
impl EventHandler<MatchEvent> for MatchEventHandler {
    async fn handle(&self, event: EventEnvelope<MatchEvent>) -> Result<(), String> {
        match &event.data {
            MatchEvent::Created(data) => {
                info!(
                    match_id = %data.match_id,
                    user1 = data.user_id_1,
                    user2 = data.user_id_2,
                    "New match created"
                );
                // Send match notification to both users
                self.handlers.send_match_notification(data.user_id_1, data.user_id_2).await?;
                self.handlers.send_match_notification(data.user_id_2, data.user_id_1).await
            }
            MatchEvent::SwipeLike(data) => {
                // Optionally notify user they were liked (for premium users)
                info!(target_user = data.target_user_id, "User liked");
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

struct ChatEventHandler {
    handlers: Arc<NotificationHandlers>,
}

#[async_trait]
impl EventHandler<ChatEvent> for ChatEventHandler {
    async fn handle(&self, event: EventEnvelope<ChatEvent>) -> Result<(), String> {
        match &event.data {
            ChatEvent::MessageSent(data) => {
                info!(
                    message_id = %data.message_id,
                    sender = data.sender_id,
                    recipient = data.recipient_id,
                    "Message sent"
                );
                self.handlers
                    .send_message_notification(
                        data.recipient_id,
                        data.sender_id,
                        data.content_preview.as_deref(),
                    )
                    .await
            }
            _ => Ok(()),
        }
    }
}

struct PaymentEventHandler {
    handlers: Arc<NotificationHandlers>,
}

#[async_trait]
impl EventHandler<PaymentEvent> for PaymentEventHandler {
    async fn handle(&self, event: EventEnvelope<PaymentEvent>) -> Result<(), String> {
        match &event.data {
            PaymentEvent::Completed(data) => {
                info!(
                    order_id = %data.order_id,
                    user_id = data.user_id,
                    amount = data.amount_cents,
                    "Payment completed"
                );
                self.handlers
                    .send_payment_success(data.user_id, &data.order_id, data.amount_cents)
                    .await
            }
            PaymentEvent::Failed(data) => {
                info!(
                    order_id = %data.order_id,
                    user_id = data.user_id,
                    error = %data.error_message,
                    "Payment failed"
                );
                self.handlers
                    .send_payment_failed(data.user_id, &data.error_message)
                    .await
            }
            PaymentEvent::SubscriptionActivated(data) => {
                info!(
                    subscription_id = %data.subscription_id,
                    user_id = data.user_id,
                    plan = %data.plan_type,
                    "Subscription activated"
                );
                self.handlers
                    .send_subscription_activated(data.user_id, &data.plan_type)
                    .await
            }
            PaymentEvent::SubscriptionCancelled(data) => {
                info!(
                    subscription_id = %data.subscription_id,
                    user_id = data.user_id,
                    "Subscription cancelled"
                );
                self.handlers.send_subscription_cancelled(data.user_id).await
            }
            _ => Ok(()),
        }
    }
}

struct NotificationCommandHandler {
    handlers: Arc<NotificationHandlers>,
}

#[async_trait]
impl EventHandler<NotificationCommand> for NotificationCommandHandler {
    async fn handle(&self, event: EventEnvelope<NotificationCommand>) -> Result<(), String> {
        match &event.data {
            NotificationCommand::SendPush(data) => {
                self.handlers
                    .send_push(data.user_id, &data.title, &data.body, data.data.as_ref())
                    .await
            }
            NotificationCommand::SendEmail(data) => {
                self.handlers
                    .send_email(data.user_id, &data.template, &data.subject, &data.variables)
                    .await
            }
            NotificationCommand::SendSms(data) => {
                self.handlers.send_sms(&data.phone_number, &data.message).await
            }
        }
    }
}
