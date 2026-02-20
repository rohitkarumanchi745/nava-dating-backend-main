//! Payment Service - Handles subscriptions and payments
//!
//! Endpoints:
//! - GET  /plans               - Get subscription plans
//! - POST /subscribe           - Create subscription
//! - GET  /subscription        - Get current subscription
//! - POST /webhook/razorpay    - Razorpay webhook
//! - POST /webhook/stripe      - Stripe webhook

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

use shared::config::{get_env, get_env_port};
use shared::events::{
    producer::{EventProducer, ProducerConfig},
    types::{
        topics, OrderCreatedData, PaymentCompletedData, PaymentEvent, PaymentFailedData,
        SubscriptionActivatedData,
    },
};
use shared::{AppError, AppResult};

struct AppState {
    db: PgPool,
    producer: EventProducer,
    razorpay_key: String,
    razorpay_secret: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let subscriber = FmtSubscriber::builder().with_max_level(Level::INFO).finish();
    tracing::subscriber::set_global_default(subscriber)?;
    dotenvy::dotenv().ok();

    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect(&get_env("DATABASE_URL"))
        .await?;

    info!("Connected to database");

    // Initialize Kafka producer
    let producer_config = ProducerConfig::from_env();
    let producer = EventProducer::new(producer_config, "payment-service")
        .expect("Failed to create Kafka producer");

    info!("Kafka producer initialized");

    let state = AppState {
        db,
        producer,
        razorpay_key: get_env("RAZORPAY_KEY_ID"),
        razorpay_secret: get_env("RAZORPAY_KEY_SECRET"),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/plans", get(get_plans))
        .route("/subscribe", post(create_subscription))
        .route("/subscription", get(get_subscription))
        .route("/webhook/razorpay", post(razorpay_webhook))
        .layer(TraceLayer::new_for_http())
        .with_state(Arc::new(state));

    let port = get_env_port("PAYMENT_SERVICE_PORT", 8005);
    info!("Payment Service starting on 0.0.0.0:{}", port);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"service": "payment-service", "status": "healthy"}))
}

#[derive(Debug, Serialize)]
struct Plan {
    id: String,
    name: String,
    price: i32,
    duration_days: i32,
    features: Vec<String>,
}

async fn get_plans() -> Json<Vec<Plan>> {
    Json(vec![
        Plan {
            id: "free".to_string(),
            name: "Free".to_string(),
            price: 0,
            duration_days: 0,
            features: vec![
                "5 likes per day".to_string(),
                "Basic matching".to_string(),
            ],
        },
        Plan {
            id: "gold".to_string(),
            name: "Gold".to_string(),
            price: 499,
            duration_days: 30,
            features: vec![
                "Unlimited likes".to_string(),
                "See who liked you".to_string(),
                "Priority matching".to_string(),
            ],
        },
        Plan {
            id: "platinum".to_string(),
            name: "Platinum".to_string(),
            price: 999,
            duration_days: 30,
            features: vec![
                "All Gold features".to_string(),
                "Profile boost".to_string(),
                "Read receipts".to_string(),
                "Advanced filters".to_string(),
            ],
        },
    ])
}

#[derive(Debug, Deserialize)]
struct SubscribeRequest {
    plan_id: String,
    payment_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct SubscribeResponse {
    subscription_id: i32,
    order_id: Option<String>,
    status: String,
}

async fn create_subscription(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SubscribeRequest>,
) -> AppResult<Json<SubscribeResponse>> {
    let user_id = 1; // Placeholder - extract from JWT

    let price = match req.plan_id.as_str() {
        "gold" => 49900,      // 499 INR in paise
        "platinum" => 99900,  // 999 INR in paise
        _ => return Err(AppError::BadRequest("Invalid plan".into())),
    };

    // Create subscription record
    let subscription_id = sqlx::query_scalar::<_, i32>(
        r#"
        INSERT INTO subscriptions (user_id, plan_type, status, amount, created_at)
        VALUES ($1, $2, 'pending', $3, NOW())
        RETURNING id
        "#
    )
    .bind(user_id)
    .bind(&req.plan_id)
    .bind(price / 100) // Store in rupees
    .fetch_one(&state.db)
    .await?;

    // In real implementation, create Razorpay order here
    let order_id = format!("order_{}", subscription_id);

    // Publish order created event to Kafka
    let event_data = OrderCreatedData {
        order_id: order_id.clone(),
        user_id,
        product_id: req.plan_id.clone(),
        amount_cents: price,
        currency: "INR".to_string(),
        gateway: "razorpay".to_string(),
    };

    if let Err(e) = state
        .producer
        .publish(
            topics::PAYMENT_EVENTS,
            PaymentEvent::OrderCreated(event_data.clone()).event_type(),
            PaymentEvent::OrderCreated(event_data),
            Some(&user_id.to_string()),
        )
        .await
    {
        error!("Failed to publish order created event: {}", e);
    }

    Ok(Json(SubscribeResponse {
        subscription_id,
        order_id: Some(order_id),
        status: "pending".to_string(),
    }))
}

#[derive(Debug, Serialize)]
struct SubscriptionInfo {
    id: i32,
    plan_type: String,
    status: String,
    expires_at: Option<String>,
}

async fn get_subscription(
    State(state): State<Arc<AppState>>,
) -> AppResult<Json<Option<SubscriptionInfo>>> {
    let user_id = 1; // Placeholder

    let subscription = sqlx::query_as::<_, (i32, String, String, Option<chrono::DateTime<chrono::Utc>>)>(
        r#"
        SELECT id, plan_type, status, expires_at
        FROM subscriptions
        WHERE user_id = $1 AND status = 'active'
        ORDER BY created_at DESC
        LIMIT 1
        "#
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;

    Ok(Json(subscription.map(|(id, plan_type, status, expires_at)| {
        SubscriptionInfo {
            id,
            plan_type,
            status,
            expires_at: expires_at.map(|t| t.to_rfc3339()),
        }
    })))
}

#[derive(Debug, Deserialize)]
struct RazorpayWebhook {
    event: String,
    payload: serde_json::Value,
}

async fn razorpay_webhook(
    State(state): State<Arc<AppState>>,
    Json(webhook): Json<RazorpayWebhook>,
) -> AppResult<Json<serde_json::Value>> {
    info!("Received webhook: {}", webhook.event);

    match webhook.event.as_str() {
        "payment.captured" => {
            // Extract payment details and update subscription
            if let Some(payment) = webhook.payload.get("payment") {
                let order_id = payment.get("order_id").and_then(|o| o.as_str()).unwrap_or("");
                let transaction_id = payment.get("id").and_then(|i| i.as_str()).unwrap_or("");
                let amount = payment.get("amount").and_then(|a| a.as_i64()).unwrap_or(0);

                // Parse subscription_id from order_id (order_{id})
                if let Some(sub_id_str) = order_id.strip_prefix("order_") {
                    if let Ok(sub_id) = sub_id_str.parse::<i32>() {
                        // Get user_id from subscription
                        let user_id = sqlx::query_scalar::<_, i32>(
                            "SELECT user_id FROM subscriptions WHERE id = $1"
                        )
                        .bind(sub_id)
                        .fetch_one(&state.db)
                        .await?;

                        // Update subscription status
                        let expires_at = Utc::now() + chrono::Duration::days(30);
                        sqlx::query(
                            r#"
                            UPDATE subscriptions
                            SET status = 'active',
                                expires_at = $2
                            WHERE id = $1
                            "#
                        )
                        .bind(sub_id)
                        .bind(expires_at)
                        .execute(&state.db)
                        .await?;

                        // Publish payment completed event
                        let event_data = PaymentCompletedData {
                            order_id: order_id.to_string(),
                            user_id,
                            transaction_id: transaction_id.to_string(),
                            amount_cents: amount,
                            currency: "INR".to_string(),
                            gateway: "razorpay".to_string(),
                            payment_method: Some("upi".to_string()),
                        };

                        if let Err(e) = state
                            .producer
                            .publish(
                                topics::PAYMENT_EVENTS,
                                PaymentEvent::Completed(event_data.clone()).event_type(),
                                PaymentEvent::Completed(event_data),
                                Some(&user_id.to_string()),
                            )
                            .await
                        {
                            error!("Failed to publish payment completed event: {}", e);
                        }

                        // Also publish subscription activated event
                        let plan_type = sqlx::query_scalar::<_, String>(
                            "SELECT plan_type FROM subscriptions WHERE id = $1"
                        )
                        .bind(sub_id)
                        .fetch_one(&state.db)
                        .await?;

                        let sub_event = SubscriptionActivatedData {
                            subscription_id: sub_id.to_string(),
                            user_id,
                            plan_type,
                            starts_at: Utc::now(),
                            expires_at,
                            gateway: "razorpay".to_string(),
                        };

                        if let Err(e) = state
                            .producer
                            .publish(
                                topics::PAYMENT_EVENTS,
                                PaymentEvent::SubscriptionActivated(sub_event.clone()).event_type(),
                                PaymentEvent::SubscriptionActivated(sub_event),
                                Some(&user_id.to_string()),
                            )
                            .await
                        {
                            error!("Failed to publish subscription activated event: {}", e);
                        }

                        info!(sub_id, user_id, "Subscription activated successfully");
                    }
                }
            }
        }
        "payment.failed" => {
            if let Some(payment) = webhook.payload.get("payment") {
                let order_id = payment.get("order_id").and_then(|o| o.as_str()).unwrap_or("");
                let error_code = payment.get("error_code").and_then(|e| e.as_str()).unwrap_or("unknown");
                let error_desc = payment.get("error_description").and_then(|e| e.as_str()).unwrap_or("Payment failed");

                if let Some(sub_id_str) = order_id.strip_prefix("order_") {
                    if let Ok(sub_id) = sub_id_str.parse::<i32>() {
                        let user_id = sqlx::query_scalar::<_, i32>(
                            "SELECT user_id FROM subscriptions WHERE id = $1"
                        )
                        .bind(sub_id)
                        .fetch_optional(&state.db)
                        .await?
                        .unwrap_or(0);

                        if user_id > 0 {
                            let event_data = PaymentFailedData {
                                order_id: order_id.to_string(),
                                user_id,
                                error_code: error_code.to_string(),
                                error_message: error_desc.to_string(),
                                gateway: "razorpay".to_string(),
                            };

                            if let Err(e) = state
                                .producer
                                .publish(
                                    topics::PAYMENT_EVENTS,
                                    PaymentEvent::Failed(event_data.clone()).event_type(),
                                    PaymentEvent::Failed(event_data),
                                    Some(&user_id.to_string()),
                                )
                                .await
                            {
                                error!("Failed to publish payment failed event: {}", e);
                            }
                        }
                    }
                }
            }
            info!("Payment failed");
        }
        _ => {
            info!("Unhandled webhook event: {}", webhook.event);
        }
    }

    Ok(Json(serde_json::json!({"status": "ok"})))
}
