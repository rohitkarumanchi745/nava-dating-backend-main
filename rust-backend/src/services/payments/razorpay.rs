//! Razorpay Payment Gateway Integration
//!
//! Documentation: https://razorpay.com/docs/api/
//! Best for: India (INR payments, UPI, Netbanking)
//! Fees: 2% per transaction

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine};
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use super::*;
use crate::error::AppError;

const RAZORPAY_API_URL: &str = "https://api.razorpay.com/v1";

/// Razorpay API client
pub struct RazorpayClient {
    client: Client,
    key_id: String,
    key_secret: String,
    auth_header: String,
}

impl RazorpayClient {
    pub fn new(key_id: String, key_secret: String) -> Self {
        let auth = format!("{}:{}", key_id, key_secret);
        let auth_header = format!("Basic {}", STANDARD.encode(auth.as_bytes()));

        Self {
            client: Client::new(),
            key_id,
            key_secret,
            auth_header,
        }
    }

    /// Get the public key ID for frontend
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    /// Verify Razorpay signature
    fn verify_signature(&self, order_id: &str, payment_id: &str, signature: &str) -> bool {
        let data = format!("{}|{}", order_id, payment_id);

        let mut mac = Hmac::<Sha256>::new_from_slice(self.key_secret.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(data.as_bytes());

        let expected = hex::encode(mac.finalize().into_bytes());
        expected == signature
    }

    /// Verify webhook signature
    fn verify_webhook_signature(&self, payload: &[u8], signature: &str) -> bool {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.key_secret.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(payload);

        let expected = hex::encode(mac.finalize().into_bytes());
        expected == signature
    }
}

// Razorpay API types
#[derive(Debug, Serialize)]
struct CreateRazorpayOrder {
    amount: i64,
    currency: String,
    receipt: String,
    notes: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct RazorpayOrder {
    id: String,
    entity: String,
    amount: i64,
    currency: String,
    receipt: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct RazorpayPayment {
    id: String,
    entity: String,
    amount: i64,
    currency: String,
    status: String,
    method: Option<String>,
    order_id: Option<String>,
    captured: bool,
    card: Option<RazorpayCard>,
    bank: Option<String>,
    wallet: Option<String>,
    vpa: Option<String>,  // UPI VPA
}

#[derive(Debug, Deserialize)]
struct RazorpayCard {
    last4: Option<String>,
    network: Option<String>,
    #[serde(rename = "type")]
    card_type: Option<String>,
}

#[derive(Debug, Serialize)]
struct CreateRazorpaySubscription {
    plan_id: String,
    total_count: i32,
    customer_notify: i32,
    notes: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct RazorpaySubscription {
    id: String,
    plan_id: String,
    status: String,
    current_start: Option<i64>,
    current_end: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct RazorpayRefund {
    id: String,
    amount: i64,
    status: String,
}

#[derive(Debug, Deserialize)]
struct RazorpayWebhookPayload {
    event: String,
    payload: serde_json::Value,
}

#[async_trait]
impl PaymentProvider for RazorpayClient {
    async fn create_order(&self, request: CreateOrderRequest) -> Result<CreateOrderResponse, AppError> {
        let order_id = format!("order_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..16].to_string());

        let razorpay_order = CreateRazorpayOrder {
            amount: request.amount_cents,
            currency: request.currency.as_str().to_string(),
            receipt: order_id.clone(),
            notes: request.metadata.map(|m| serde_json::to_value(m).unwrap_or_default()),
        };

        let response = self.client
            .post(format!("{}/orders", RAZORPAY_API_URL))
            .header("Authorization", &self.auth_header)
            .json(&razorpay_order)
            .send()
            .await
            .map_err(|e| AppError::internal(format!("Razorpay request failed: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AppError::internal(format!("Razorpay error: {}", error_text)));
        }

        let rz_order: RazorpayOrder = response.json().await
            .map_err(|e| AppError::internal(format!("Failed to parse Razorpay response: {}", e)))?;

        Ok(CreateOrderResponse {
            order_id,
            gateway: PaymentGateway::Razorpay,
            gateway_order_id: rz_order.id,
            amount_cents: rz_order.amount,
            currency: rz_order.currency,
            razorpay_key_id: Some(self.key_id.clone()),
            stripe_client_secret: None,
            stripe_publishable_key: None,
        })
    }

    async fn verify_payment(&self, request: VerifyPaymentRequest) -> Result<PaymentVerification, AppError> {
        let payment_id = request.razorpay_payment_id
            .ok_or_else(|| AppError::bad_request("razorpay_payment_id required"))?;
        let signature = request.razorpay_signature
            .ok_or_else(|| AppError::bad_request("razorpay_signature required"))?;

        // Fetch the order to get the gateway_order_id
        // In production, you'd look this up from your database
        let gateway_order_id = request.order_id.clone(); // This should come from DB

        // Verify signature
        if !self.verify_signature(&gateway_order_id, &payment_id, &signature) {
            return Ok(PaymentVerification {
                success: false,
                order_id: request.order_id,
                transaction_id: payment_id,
                amount_cents: 0,
                currency: "INR".to_string(),
                payment_method: None,
                error_message: Some("Invalid signature".to_string()),
            });
        }

        // Fetch payment details
        let response = self.client
            .get(format!("{}/payments/{}", RAZORPAY_API_URL, payment_id))
            .header("Authorization", &self.auth_header)
            .send()
            .await
            .map_err(|e| AppError::internal(format!("Razorpay request failed: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AppError::internal(format!("Razorpay error: {}", error_text)));
        }

        let payment: RazorpayPayment = response.json().await
            .map_err(|e| AppError::internal(format!("Failed to parse payment: {}", e)))?;

        let payment_method = payment.method.clone().or_else(|| {
            if payment.vpa.is_some() { Some("upi".to_string()) }
            else if payment.card.is_some() { Some("card".to_string()) }
            else if payment.bank.is_some() { Some("netbanking".to_string()) }
            else if payment.wallet.is_some() { Some("wallet".to_string()) }
            else { None }
        });

        Ok(PaymentVerification {
            success: payment.status == "captured" || payment.captured,
            order_id: request.order_id,
            transaction_id: payment.id,
            amount_cents: payment.amount,
            currency: payment.currency,
            payment_method,
            error_message: if payment.status != "captured" && !payment.captured {
                Some(format!("Payment status: {}", payment.status))
            } else {
                None
            },
        })
    }

    async fn create_subscription(&self, request: CreateSubscriptionRequest) -> Result<SubscriptionResponse, AppError> {
        // In production, you'd look up the Razorpay plan_id from your products table
        let plan_id = format!("plan_{}", request.product_id);

        let subscription = CreateRazorpaySubscription {
            plan_id,
            total_count: 12, // 12 billing cycles
            customer_notify: 1,
            notes: None,
        };

        let response = self.client
            .post(format!("{}/subscriptions", RAZORPAY_API_URL))
            .header("Authorization", &self.auth_header)
            .json(&subscription)
            .send()
            .await
            .map_err(|e| AppError::internal(format!("Razorpay request failed: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AppError::internal(format!("Razorpay error: {}", error_text)));
        }

        let rz_sub: RazorpaySubscription = response.json().await
            .map_err(|e| AppError::internal(format!("Failed to parse subscription: {}", e)))?;

        Ok(SubscriptionResponse {
            subscription_id: format!("sub_{}", uuid::Uuid::new_v4()),
            gateway: PaymentGateway::Razorpay,
            gateway_subscription_id: rz_sub.id.clone(),
            status: rz_sub.status,
            current_period_start: rz_sub.current_start.unwrap_or(0),
            current_period_end: rz_sub.current_end.unwrap_or(0),
            razorpay_subscription_id: Some(rz_sub.id),
            stripe_subscription_id: None,
        })
    }

    async fn cancel_subscription(&self, subscription_id: &str) -> Result<(), AppError> {
        let response = self.client
            .post(format!("{}/subscriptions/{}/cancel", RAZORPAY_API_URL, subscription_id))
            .header("Authorization", &self.auth_header)
            .send()
            .await
            .map_err(|e| AppError::internal(format!("Razorpay request failed: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AppError::internal(format!("Razorpay error: {}", error_text)));
        }

        Ok(())
    }

    async fn create_refund(&self, payment_id: &str, amount_cents: Option<i64>) -> Result<String, AppError> {
        let mut body = serde_json::json!({});
        if let Some(amount) = amount_cents {
            body["amount"] = serde_json::json!(amount);
        }

        let response = self.client
            .post(format!("{}/payments/{}/refund", RAZORPAY_API_URL, payment_id))
            .header("Authorization", &self.auth_header)
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::internal(format!("Razorpay request failed: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AppError::internal(format!("Razorpay error: {}", error_text)));
        }

        let refund: RazorpayRefund = response.json().await
            .map_err(|e| AppError::internal(format!("Failed to parse refund: {}", e)))?;

        Ok(refund.id)
    }

    fn parse_webhook(&self, payload: &[u8], signature: &str) -> Result<WebhookEvent, AppError> {
        // Verify webhook signature
        if !self.verify_webhook_signature(payload, signature) {
            return Err(AppError::unauthorized("Invalid webhook signature"));
        }

        let webhook: RazorpayWebhookPayload = serde_json::from_slice(payload)
            .map_err(|e| AppError::bad_request(format!("Invalid webhook payload: {}", e)))?;

        let event_type = match webhook.event.as_str() {
            "payment.captured" | "order.paid" => WebhookEventType::OrderPaid,
            "payment.failed" => WebhookEventType::OrderFailed,
            "subscription.activated" => WebhookEventType::SubscriptionActivated,
            "subscription.charged" => WebhookEventType::SubscriptionCharged,
            "subscription.cancelled" => WebhookEventType::SubscriptionCancelled,
            "subscription.completed" | "subscription.expired" => WebhookEventType::SubscriptionExpired,
            "refund.created" => WebhookEventType::RefundCreated,
            "refund.processed" => WebhookEventType::RefundProcessed,
            _ => return Err(AppError::bad_request(format!("Unknown event: {}", webhook.event))),
        };

        Ok(WebhookEvent {
            event_type,
            gateway: PaymentGateway::Razorpay,
            gateway_event_id: webhook.event.clone(),
            data: webhook.payload,
        })
    }
}
