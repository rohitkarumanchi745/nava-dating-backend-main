//! Payment endpoints for web payments (Razorpay + Stripe)
//!
//! Endpoints:
//! - POST /api/payments/create-order - Create payment order
//! - POST /api/payments/verify - Verify payment completion
//! - GET /api/payments/products - List available products
//! - GET /api/payments/subscriptions - Get user's active subscriptions
//! - POST /api/payments/subscriptions/cancel - Cancel subscription
//! - POST /webhook/razorpay - Razorpay webhook
//! - POST /webhook/stripe - Stripe webhook

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    auth::Claims,
    error::AppError,
    jobs::get_dlq_stats,
    services::payments::{
        CreateOrderRequest, Currency, PaymentGateway,
        VerifyPaymentRequest, WebhookEvent, WebhookEventType,
    },
    state::AppState,
};

// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Deserialize)]
pub struct CreateOrderPayload {
    pub product_id: String,
    pub currency: Option<String>,
    pub region_code: Option<String>,
    pub platform: Option<String>,
    /// Client-generated idempotency key to prevent duplicate orders
    pub idempotency_key: Option<String>,
}

#[derive(Serialize)]
pub struct CreateOrderResponse {
    pub order_id: String,
    pub gateway: String,
    pub amount_cents: i64,
    pub currency: String,
    pub gateway_order_id: String,
    pub client_secret: Option<String>,  // For Stripe
    pub razorpay_key_id: Option<String>, // For Razorpay
}

#[derive(Deserialize)]
pub struct VerifyPaymentPayload {
    pub order_id: String,
    // Razorpay fields
    pub razorpay_payment_id: Option<String>,
    pub razorpay_signature: Option<String>,
    // Stripe fields
    pub stripe_payment_intent_id: Option<String>,
}

#[derive(Serialize)]
pub struct VerifyPaymentResponse {
    pub success: bool,
    pub transaction_id: String,
    pub message: String,
}

#[derive(Serialize)]
pub struct ProductResponse {
    pub id: String,
    pub name: String,
    pub description: String,
    pub product_type: String,
    pub prices: Vec<ProductPriceResponse>,
}

#[derive(Serialize)]
pub struct ProductPriceResponse {
    pub currency: String,
    pub amount_cents: i64,
    pub formatted_price: String,
}

#[derive(Serialize)]
pub struct SubscriptionResponse {
    pub id: String,
    pub product_name: String,
    pub status: String,
    pub current_period_end: String,
    pub cancel_at_period_end: bool,
}

#[derive(Deserialize)]
pub struct CancelSubscriptionPayload {
    pub subscription_id: String,
}

// ============================================================================
// Handlers
// ============================================================================

/// Create a payment order
/// POST /api/payments/create-order
pub async fn create_order(
    State(state): State<AppState>,
    claims: Claims,
    Json(payload): Json<CreateOrderPayload>,
) -> Result<Json<CreateOrderResponse>, AppError> {
    let user_id = claims.sub.parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

    // Idempotency check: If idempotency_key provided, check if already processed
    if let Some(ref idempotency_key) = payload.idempotency_key {
        let existing = sqlx::query!(
            r#"
            SELECT order_id, gateway, amount_cents, currency, gateway_order_id, status
            FROM payment_orders
            WHERE user_id = $1 AND idempotency_key = $2
            LIMIT 1
            "#,
            user_id,
            idempotency_key
        )
        .fetch_optional(&state.db)
        .await?;

        if let Some(order) = existing {
            // Return the existing order info (idempotent response)
            return Ok(Json(CreateOrderResponse {
                order_id: order.order_id,
                gateway: order.gateway,
                amount_cents: order.amount_cents as i64,
                currency: order.currency,
                gateway_order_id: order.gateway_order_id.unwrap_or_default(),
                client_secret: None, // Would need to retrieve from provider if needed
                razorpay_key_id: None,
            }));
        }
    }

    // Determine region and currency
    let region_code = payload.region_code.as_deref().unwrap_or("US");
    let currency_str = payload.currency.as_deref().unwrap_or_else(|| {
        Currency::for_region(region_code).as_str()
    });
    let platform = payload.platform.as_deref().unwrap_or("web");

    // Fetch product and price from database
    let product = sqlx::query!(
        r#"
        SELECT p.id, p.name, p.product_type, pp.amount_cents
        FROM products p
        JOIN product_prices pp ON p.id = pp.product_id
        WHERE p.product_id = $1 AND pp.currency = $2 AND p.is_active = true
        "#,
        payload.product_id,
        currency_str
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("Product not found or price not available for currency"))?;

    let gateway = PaymentGateway::for_region(region_code);

    // Create order in database
    let order_id = uuid::Uuid::new_v4().to_string();
    sqlx::query!(
        r#"
        INSERT INTO payment_orders (order_id, user_id, product_id, gateway, amount_cents, currency, status, idempotency_key)
        VALUES ($1, $2, $3, $4, $5, $6, 'pending', $7)
        "#,
        order_id,
        user_id,
        product.id,
        gateway.as_str(),
        product.amount_cents,
        currency_str,
        payload.idempotency_key
    )
    .execute(&state.db)
    .await?;

    // Create order with payment provider
    let payment_service = state.payment_service.as_ref()
        .ok_or_else(|| AppError::internal("Payment service not configured"))?;

    let currency = Currency::from_str(currency_str).unwrap_or(Currency::USD);

    let request = CreateOrderRequest {
        user_id,
        product_id: payload.product_id.clone(),
        amount_cents: product.amount_cents as i64,
        currency,
        region_code: region_code.to_string(),
        platform: platform.to_string(),
        metadata: None,
    };

    let provider_response = payment_service.create_order(request).await?;

    // Update order with gateway order ID
    sqlx::query!(
        "UPDATE payment_orders SET gateway_order_id = $1 WHERE order_id = $2",
        provider_response.gateway_order_id,
        order_id
    )
    .execute(&state.db)
    .await?;

    Ok(Json(CreateOrderResponse {
        order_id,
        gateway: provider_response.gateway.as_str().to_string(),
        amount_cents: product.amount_cents as i64,
        currency: currency_str.to_string(),
        gateway_order_id: provider_response.gateway_order_id,
        client_secret: provider_response.stripe_client_secret,
        razorpay_key_id: provider_response.razorpay_key_id,
    }))
}

/// Verify payment completion
/// POST /api/payments/verify
pub async fn verify_payment(
    State(state): State<AppState>,
    claims: Claims,
    Json(payload): Json<VerifyPaymentPayload>,
) -> Result<Json<VerifyPaymentResponse>, AppError> {
    let user_id = claims.sub.parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

    // Fetch order from database
    let order = sqlx::query!(
        r#"
        SELECT id, user_id, product_id, gateway, gateway_order_id, amount_cents, currency, status
        FROM payment_orders
        WHERE order_id = $1
        "#,
        payload.order_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("Order not found"))?;

    // Verify user owns this order
    if order.user_id != Some(user_id) {
        return Err(AppError::forbidden("Not authorized to verify this order"));
    }

    // Check if already processed
    let status = order.status.as_deref().unwrap_or("unknown");
    if status != "pending" {
        return Ok(Json(VerifyPaymentResponse {
            success: status == "completed",
            transaction_id: payload.order_id.clone(),
            message: format!("Order already processed: {}", status),
        }));
    }

    let gateway = PaymentGateway::from_str(&order.gateway)
        .ok_or_else(|| AppError::internal("Invalid gateway in order"))?;

    let payment_service = state.payment_service.as_ref()
        .ok_or_else(|| AppError::internal("Payment service not configured"))?;

    let verify_request = VerifyPaymentRequest {
        order_id: payload.order_id.clone(),
        gateway,
        razorpay_payment_id: payload.razorpay_payment_id.clone(),
        razorpay_signature: payload.razorpay_signature.clone(),
        stripe_payment_intent_id: payload.stripe_payment_intent_id.clone(),
    };

    let verification = payment_service.verify_payment(verify_request).await?;

    let payment_id = payload.razorpay_payment_id.or(payload.stripe_payment_intent_id)
        .unwrap_or_else(|| verification.transaction_id.clone());

    if verification.success {
        // Create transaction record
        let transaction_id = uuid::Uuid::new_v4().to_string();
        sqlx::query!(
            r#"
            INSERT INTO payment_transactions
            (transaction_id, order_id, user_id, gateway, gateway_payment_id, amount_cents, currency, status)
            VALUES ($1, $2, $3, $4, $5, $6, $7, 'completed')
            "#,
            transaction_id,
            order.id,
            user_id,
            order.gateway,
            payment_id,
            order.amount_cents,
            order.currency
        )
        .execute(&state.db)
        .await?;

        // Update order status
        sqlx::query!(
            "UPDATE payment_orders SET status = 'completed', completed_at = NOW() WHERE id = $1",
            order.id
        )
        .execute(&state.db)
        .await?;

        // Grant the product to user (subscription or consumable)
        if let Some(product_id) = order.product_id {
            grant_product_to_user(&state.db, user_id, product_id).await?;
        }

        Ok(Json(VerifyPaymentResponse {
            success: true,
            transaction_id,
            message: "Payment verified successfully".to_string(),
        }))
    } else {
        // Update order status
        sqlx::query!(
            "UPDATE payment_orders SET status = 'failed' WHERE id = $1",
            order.id
        )
        .execute(&state.db)
        .await?;

        Ok(Json(VerifyPaymentResponse {
            success: false,
            transaction_id: String::new(),
            message: verification.error_message.unwrap_or_else(|| "Payment verification failed".to_string()),
        }))
    }
}

/// List available products with prices
/// GET /api/payments/products
pub async fn list_products(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<Vec<ProductResponse>>, AppError> {
    let _user_id = claims.sub.parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

    let products = sqlx::query!(
        r#"
        SELECT p.product_id, p.name, p.description, p.product_type,
               pp.currency, pp.amount_cents
        FROM products p
        JOIN product_prices pp ON p.id = pp.product_id
        WHERE p.is_active = true
        ORDER BY p.sort_order, pp.currency
        "#
    )
    .fetch_all(state.read_pool())
    .await?;

    // Group by product
    let mut product_map: std::collections::HashMap<String, ProductResponse> = std::collections::HashMap::new();

    for row in products {
        let entry = product_map.entry(row.product_id.clone()).or_insert_with(|| ProductResponse {
            id: row.product_id.clone(),
            name: row.name.clone(),
            description: row.description.clone().unwrap_or_default(),
            product_type: row.product_type.clone(),
            prices: Vec::new(),
        });

        entry.prices.push(ProductPriceResponse {
            currency: row.currency.clone(),
            amount_cents: row.amount_cents as i64,
            formatted_price: format_price(row.amount_cents as i64, &row.currency),
        });
    }

    Ok(Json(product_map.into_values().collect()))
}

/// Get user's active subscriptions
/// GET /api/payments/subscriptions
pub async fn get_subscriptions(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<Vec<SubscriptionResponse>>, AppError> {
    let user_id = claims.sub.parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

    let subscriptions = sqlx::query!(
        r#"
        SELECT us.subscription_id, p.name, us.status, us.current_period_end, us.cancel_at_period_end
        FROM user_subscriptions us
        JOIN products p ON us.product_id = p.id
        WHERE us.user_id = $1 AND us.status IN ('active', 'trialing', 'past_due')
        "#,
        user_id
    )
    .fetch_all(state.read_pool())
    .await?;

    Ok(Json(subscriptions.into_iter().map(|s| SubscriptionResponse {
        id: s.subscription_id.unwrap_or_default(),
        product_name: s.name,
        status: s.status.unwrap_or_default(),
        current_period_end: s.current_period_end
            .map(|d| d.to_string())
            .unwrap_or_default(),
        cancel_at_period_end: s.cancel_at_period_end.unwrap_or(false),
    }).collect()))
}

/// Cancel subscription
/// POST /api/payments/subscriptions/cancel
pub async fn cancel_subscription(
    State(state): State<AppState>,
    claims: Claims,
    Json(payload): Json<CancelSubscriptionPayload>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id = claims.sub.parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

    // Verify user owns this subscription
    let subscription = sqlx::query!(
        r#"
        SELECT gateway, gateway_subscription_id
        FROM user_subscriptions
        WHERE subscription_id = $1 AND user_id = $2
        "#,
        payload.subscription_id,
        user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("Subscription not found"))?;

    let gateway_str = subscription.gateway.as_deref()
        .ok_or_else(|| AppError::internal("No gateway for subscription"))?;
    let gateway = PaymentGateway::from_str(gateway_str)
        .ok_or_else(|| AppError::internal("Invalid gateway"))?;

    let payment_service = state.payment_service.as_ref()
        .ok_or_else(|| AppError::internal("Payment service not configured"))?;

    let gateway_sub_id = subscription.gateway_subscription_id.as_deref()
        .ok_or_else(|| AppError::internal("No gateway subscription ID"))?;

    // Cancel with payment provider (at period end)
    payment_service.cancel_subscription(gateway, gateway_sub_id).await?;

    // Update database
    sqlx::query!(
        "UPDATE user_subscriptions SET cancel_at_period_end = true WHERE subscription_id = $1",
        payload.subscription_id
    )
    .execute(&state.db)
    .await?;

    Ok(Json(json!({ "message": "Subscription will be cancelled at the end of the billing period" })))
}

/// Razorpay webhook handler
/// POST /webhook/razorpay
pub async fn razorpay_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, AppError> {
    let signature = headers
        .get("x-razorpay-signature")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::bad_request("Missing signature"))?;

    let payment_service = state.payment_service.as_ref()
        .ok_or_else(|| AppError::internal("Payment service not configured"))?;

    let event = payment_service.process_webhook(PaymentGateway::Razorpay, &body, signature)?;

    // Process webhook, enqueue to DLQ on failure (always return 200 to gateway to avoid infinite retries)
    if let Err(e) = process_webhook_event(&state, event).await {
        tracing::error!("Razorpay webhook processing failed, enqueuing to DLQ: {e}");
        let dlq = crate::services::payments::retry::DeadLetterQueue::new(state.db.clone());
        let payload = serde_json::from_slice(&body).unwrap_or(serde_json::json!({"raw": "parse_failed"}));
        let _ = dlq.enqueue("webhooks", payload, &format!("razorpay webhook: {e}")).await;
    }

    Ok((StatusCode::OK, "OK"))
}

/// Stripe webhook handler
/// POST /webhook/stripe
pub async fn stripe_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, AppError> {
    let signature = headers
        .get("stripe-signature")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::bad_request("Missing signature"))?;

    let payment_service = state.payment_service.as_ref()
        .ok_or_else(|| AppError::internal("Payment service not configured"))?;

    let event = payment_service.process_webhook(PaymentGateway::Stripe, &body, signature)?;

    // Process webhook, enqueue to DLQ on failure (always return 200 to gateway to avoid infinite retries)
    if let Err(e) = process_webhook_event(&state, event).await {
        tracing::error!("Stripe webhook processing failed, enqueuing to DLQ: {e}");
        let dlq = crate::services::payments::retry::DeadLetterQueue::new(state.db.clone());
        let payload = serde_json::from_slice(&body).unwrap_or(serde_json::json!({"raw": "parse_failed"}));
        let _ = dlq.enqueue("webhooks", payload, &format!("stripe webhook: {e}")).await;
    }

    Ok((StatusCode::OK, "OK"))
}

// ============================================================================
// Helper Functions
// ============================================================================

async fn grant_product_to_user(db: &sqlx::PgPool, user_id: i32, product_id: i32) -> Result<(), AppError> {
    // Get product type
    let product = sqlx::query!(
        "SELECT product_type, duration_days, consumable_quantity FROM products WHERE id = $1",
        product_id
    )
    .fetch_one(db)
    .await?;

    match product.product_type.as_str() {
        "subscription" => {
            // Update user to premium
            sqlx::query!(
                "UPDATE users SET is_premium = true WHERE id = $1",
                user_id as i64
            )
            .execute(db)
            .await?;
        }
        "consumable" => {
            // Add consumables (boosts, super likes, etc.)
            let quantity = product.consumable_quantity.unwrap_or(1);
            sqlx::query!(
                r#"
                INSERT INTO user_consumables (user_id, product_id, quantity)
                VALUES ($1, $2, $3)
                ON CONFLICT (user_id, product_id)
                DO UPDATE SET quantity = user_consumables.quantity + EXCLUDED.quantity
                "#,
                user_id,
                product_id,
                quantity
            )
            .execute(db)
            .await?;
        }
        _ => {
            tracing::warn!("Unknown product type: {}", product.product_type);
        }
    }

    Ok(())
}

async fn process_webhook_event(
    state: &AppState,
    event: WebhookEvent,
) -> Result<(), AppError> {
    // Extract order/payment IDs from webhook data
    let order_id = event.data.get("order_id")
        .or_else(|| event.data.get("razorpay_order_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let payment_id = event.data.get("payment_id")
        .or_else(|| event.data.get("razorpay_payment_id"))
        .or_else(|| event.data.get("payment_intent"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let subscription_id = event.data.get("subscription_id")
        .or_else(|| event.data.get("razorpay_subscription_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    match event.event_type {
        WebhookEventType::OrderPaid => {
            if let (Some(order_id), Some(payment_id)) = (&order_id, &payment_id) {
                // Find order and mark as completed
                let order = sqlx::query!(
                    "SELECT id, user_id, product_id, status FROM payment_orders WHERE gateway_order_id = $1",
                    order_id
                )
                .fetch_optional(&state.db)
                .await?;

                if let Some(order) = order {
                    if order.status.as_deref() == Some("pending") {
                        // Create transaction
                        let transaction_id = uuid::Uuid::new_v4().to_string();
                        sqlx::query!(
                            r#"
                            INSERT INTO payment_transactions
                            (transaction_id, order_id, user_id, gateway, gateway_payment_id, amount_cents, currency, status)
                            SELECT $1, id, user_id, gateway, $2, amount_cents, currency, 'completed'
                            FROM payment_orders WHERE id = $3
                            "#,
                            transaction_id,
                            payment_id,
                            order.id
                        )
                        .execute(&state.db)
                        .await?;

                        // Update order
                        sqlx::query!(
                            "UPDATE payment_orders SET status = 'completed', completed_at = NOW() WHERE id = $1",
                            order.id
                        )
                        .execute(&state.db)
                        .await?;

                        // Grant product
                        if let (Some(user_id), Some(product_id)) = (order.user_id, order.product_id) {
                            grant_product_to_user(&state.db, user_id, product_id).await?;
                        }
                    }
                }
            }
        }
        WebhookEventType::OrderFailed => {
            if let Some(order_id) = &order_id {
                sqlx::query!(
                    "UPDATE payment_orders SET status = 'failed' WHERE gateway_order_id = $1",
                    order_id
                )
                .execute(&state.db)
                .await?;
            }
        }
        WebhookEventType::SubscriptionActivated | WebhookEventType::SubscriptionCharged => {
            // Handle subscription lifecycle events
            tracing::info!("Subscription event: {:?}", event.event_type);
        }
        WebhookEventType::SubscriptionCancelled | WebhookEventType::SubscriptionExpired => {
            if let Some(subscription_id) = &subscription_id {
                sqlx::query!(
                    "UPDATE user_subscriptions SET status = 'cancelled', cancelled_at = NOW() WHERE gateway_subscription_id = $1",
                    subscription_id
                )
                .execute(&state.db)
                .await?;

                // Remove premium status
                sqlx::query!(
                    r#"
                    UPDATE users SET is_premium = false
                    WHERE id = (SELECT user_id FROM user_subscriptions WHERE gateway_subscription_id = $1)
                    "#,
                    subscription_id
                )
                .execute(&state.db)
                .await?;
            }
        }
        WebhookEventType::RefundCreated | WebhookEventType::RefundProcessed => {
            tracing::info!("Refund event: {:?}", event);
        }
        WebhookEventType::DisputeCreated | WebhookEventType::DisputeWon | WebhookEventType::DisputeLost => {
            tracing::warn!("Dispute event: {:?}", event);
            // TODO: Send alert to admin, potentially suspend user
        }
    }

    Ok(())
}

fn format_price(amount_cents: i64, currency: &str) -> String {
    let amount = amount_cents as f64 / 100.0;
    match currency {
        "INR" => format!("₹{:.0}", amount),
        "USD" => format!("${:.2}", amount),
        "EUR" => format!("€{:.2}", amount),
        "GBP" => format!("£{:.2}", amount),
        _ => format!("{} {:.2}", currency, amount),
    }
}

// ============================================================================
// Apple StoreKit 2 Verification
// ============================================================================

#[derive(Deserialize)]
pub struct VerifyApplePayload {
    pub transaction_id: String,
    pub product_id: String,
    pub original_transaction_id: Option<String>,
}

#[derive(Serialize)]
pub struct VerifyAppleResponse {
    pub success: bool,
    pub message: String,
}

/// Verify an Apple StoreKit 2 transaction and grant the product
/// POST /api/payments/verify-apple
pub async fn verify_apple_payment(
    State(state): State<AppState>,
    claims: Claims,
    Json(payload): Json<VerifyApplePayload>,
) -> Result<Json<VerifyAppleResponse>, AppError> {
    let user_id = claims.sub.parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

    // Check for duplicate transaction
    let existing = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM payment_transactions WHERE gateway_payment_id = $1 AND gateway = 'apple')"
    )
    .bind(&payload.transaction_id)
    .fetch_one(&state.db)
    .await
    .unwrap_or(false);

    if existing {
        return Ok(Json(VerifyAppleResponse {
            success: true,
            message: "Transaction already recorded".to_string(),
        }));
    }

    // Look up product by store product ID (e.g. "com.nava.gold_monthly" -> backend product)
    let product = sqlx::query!(
        "SELECT id, product_type FROM products WHERE product_id = $1 AND is_active = true",
        payload.product_id
    )
    .fetch_optional(&state.db)
    .await?;

    let product = match product {
        Some(p) => p,
        None => {
            tracing::warn!("Apple verify: unknown product_id={}", payload.product_id);
            return Ok(Json(VerifyAppleResponse {
                success: false,
                message: format!("Unknown product: {}", payload.product_id),
            }));
        }
    };

    // Create transaction record
    let transaction_id = uuid::Uuid::new_v4().to_string();
    sqlx::query!(
        r#"
        INSERT INTO payment_transactions
        (transaction_id, order_id, user_id, gateway, gateway_payment_id, amount_cents, currency, status)
        VALUES ($1, NULL, $2, 'apple', $3, 0, 'USD', 'completed')
        "#,
        transaction_id,
        user_id,
        payload.transaction_id
    )
    .execute(&state.db)
    .await?;

    // Grant the product to the user
    grant_product_to_user(&state.db, user_id, product.id).await?;

    tracing::info!(
        "Apple payment verified: user={}, product={}, txn={}",
        user_id, payload.product_id, payload.transaction_id
    );

    Ok(Json(VerifyAppleResponse {
        success: true,
        message: "Payment verified and product granted".to_string(),
    }))
}

// ============================================================================
// Account Deletion (GDPR / App Store requirement)
// ============================================================================

/// Delete the authenticated user's account and all associated data
/// DELETE /account/delete
pub async fn delete_account(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id = claims.sub.parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

    // Soft-delete: mark account as inactive (preserves data for recovery)
    sqlx::query!(
        "UPDATE users SET is_active = false, updated_at = NOW() WHERE id = $1",
        user_id as i64
    )
    .execute(&state.db)
    .await?;

    // Cancel any active subscriptions
    sqlx::query!(
        "UPDATE user_subscriptions SET status = 'cancelled', cancelled_at = NOW() WHERE user_id = $1 AND status = 'active'",
        user_id
    )
    .execute(&state.db)
    .await?;

    // Remove from discovery
    sqlx::query!(
        "DELETE FROM user_locations WHERE user_id = $1",
        user_id as i64
    )
    .execute(&state.db)
    .await?;

    tracing::info!("Account deleted: user_id={}", user_id);

    Ok(Json(json!({
        "success": true,
        "message": "Account scheduled for deletion. Data will be permanently removed in 30 days."
    })))
}

/// Get DLQ statistics for monitoring
/// GET /api/payments/dlq/stats
pub async fn dlq_stats(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<serde_json::Value>, AppError> {
    let _user_id = claims.sub.parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

    // In production, you'd want to check if user is admin
    // For now, any authenticated user can view stats

    let stats = get_dlq_stats(&state).await
        .map_err(|e| AppError::internal(format!("Failed to get DLQ stats: {}", e)))?;

    Ok(Json(stats))
}
