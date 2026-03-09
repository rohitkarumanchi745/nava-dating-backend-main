//! Stripe Payment Gateway Integration
//!
//! Documentation: https://stripe.com/docs/api
//! Best for: Global payments (US, EU, UK, etc.)
//! Fees: 2.9% + 30¢ per transaction

use async_trait::async_trait;
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde::Deserialize;
use sha2::Sha256;

use super::*;
use crate::error::AppError;

const STRIPE_API_URL: &str = "https://api.stripe.com/v1";

/// Stripe API client
pub struct StripeClient {
    client: Client,
    secret_key: String,
    publishable_key: String,
    webhook_secret: Option<String>,
}

impl StripeClient {
    pub fn new(secret_key: String, publishable_key: String, webhook_secret: Option<String>) -> Self {
        Self {
            client: Client::new(),
            secret_key,
            publishable_key,
            webhook_secret,
        }
    }

    /// Get publishable key for frontend
    pub fn publishable_key(&self) -> &str {
        &self.publishable_key
    }

    /// Verify Stripe webhook signature
    fn verify_webhook_signature(&self, payload: &[u8], signature_header: &str, timestamp: i64) -> bool {
        let secret = match &self.webhook_secret {
            Some(s) => s,
            None => return false,
        };

        // Parse the signature header
        let parts: std::collections::HashMap<&str, &str> = signature_header
            .split(',')
            .filter_map(|part| {
                let mut split = part.split('=');
                Some((split.next()?, split.next()?))
            })
            .collect();

        let v1_signature = match parts.get("v1") {
            Some(sig) => *sig,
            None => return false,
        };

        // Compute expected signature
        let signed_payload = format!("{}.{}", timestamp, String::from_utf8_lossy(payload));

        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(signed_payload.as_bytes());

        let expected = hex::encode(mac.finalize().into_bytes());
        expected == v1_signature
    }

    /// Make authenticated request to Stripe with retry and jitter
    async fn stripe_request<T: for<'de> Deserialize<'de> + Send + 'static>(
        &self,
        method: reqwest::Method,
        endpoint: &str,
        body: Option<&[(&str, &str)]>,
    ) -> Result<T, AppError> {
        use super::retry::{RetryConfig, with_retry};

        let url = format!("{}{}", STRIPE_API_URL, endpoint);
        let config = RetryConfig::for_payments();

        // Clone what we need for the retry closure
        let client = self.client.clone();
        let secret_key = self.secret_key.clone();
        let method_clone = method.clone();
        let body_owned: Option<Vec<(String, String)>> = body.map(|b| {
            b.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
        });

        let result = with_retry(&config, &format!("stripe_{}", endpoint), || {
            let url = url.clone();
            let client = client.clone();
            let secret_key = secret_key.clone();
            let method = method_clone.clone();
            let body = body_owned.clone();

            async move {
                let mut request = client
                    .request(method, &url)
                    .bearer_auth(&secret_key);

                if let Some(ref form_data) = body {
                    let form: Vec<(&str, &str)> = form_data.iter()
                        .map(|(k, v)| (k.as_str(), v.as_str()))
                        .collect();
                    request = request.form(&form);
                }

                let response = request
                    .send()
                    .await
                    .map_err(|e| AppError::internal(format!("Stripe request failed: {}", e)))?;

                let status = response.status();

                // Check for retryable HTTP status codes
                if status.as_u16() == 429 || status.as_u16() >= 500 {
                    return Err(AppError::internal(format!(
                        "Stripe temporarily unavailable ({})",
                        status
                    )));
                }

                if !status.is_success() {
                    let error: StripeError = response.json().await
                        .unwrap_or(StripeError {
                            error: StripeErrorDetail {
                                message: "Unknown error".to_string(),
                                error_type: "api_error".to_string(),
                                code: None,
                            },
                        });
                    return Err(AppError::internal(format!("Stripe error: {}", error.error.message)));
                }

                response.json::<T>().await
                    .map_err(|e| AppError::internal(format!("Failed to parse Stripe response: {}", e)))
            }
        }).await?;

        Ok(result.value)
    }
}

// Stripe API types
#[derive(Debug, Deserialize)]
struct StripeError {
    error: StripeErrorDetail,
}

#[derive(Debug, Deserialize)]
struct StripeErrorDetail {
    message: String,
    #[serde(rename = "type")]
    error_type: String,
    code: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StripePaymentIntent {
    id: String,
    client_secret: String,
    amount: i64,
    currency: String,
    status: String,
    payment_method: Option<String>,
    metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct StripeSubscription {
    id: String,
    status: String,
    current_period_start: i64,
    current_period_end: i64,
    items: StripeSubscriptionItems,
}

#[derive(Debug, Deserialize)]
struct StripeSubscriptionItems {
    data: Vec<StripeSubscriptionItem>,
}

#[derive(Debug, Deserialize)]
struct StripeSubscriptionItem {
    id: String,
    price: StripePrice,
}

#[derive(Debug, Deserialize)]
struct StripePrice {
    id: String,
    unit_amount: i64,
    currency: String,
}

#[derive(Debug, Deserialize)]
struct StripeRefund {
    id: String,
    amount: i64,
    status: String,
}

#[derive(Debug, Deserialize)]
struct StripePaymentMethod {
    id: String,
    #[serde(rename = "type")]
    payment_type: String,
    card: Option<StripeCard>,
}

#[derive(Debug, Deserialize)]
struct StripeCard {
    last4: String,
    brand: String,
    exp_month: i32,
    exp_year: i32,
}

#[derive(Debug, Deserialize)]
struct StripeWebhookEvent {
    id: String,
    #[serde(rename = "type")]
    event_type: String,
    data: StripeWebhookData,
    created: i64,
}

#[derive(Debug, Deserialize)]
struct StripeWebhookData {
    object: serde_json::Value,
}

#[async_trait]
impl PaymentProvider for StripeClient {
    async fn create_order(&self, request: CreateOrderRequest) -> Result<CreateOrderResponse, AppError> {
        let order_id = format!("order_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..16].to_string());

        let amount_str = request.amount_cents.to_string();
        let currency = request.currency.as_str().to_lowercase();
        let user_id_str = request.user_id.to_string();

        let form_data = vec![
            ("amount", amount_str.as_str()),
            ("currency", &currency),
            ("automatic_payment_methods[enabled]", "true"),
            ("metadata[order_id]", &order_id),
            ("metadata[user_id]", user_id_str.as_str()),
            ("metadata[product_id]", &request.product_id),
        ];

        let payment_intent: StripePaymentIntent = self.stripe_request(
            reqwest::Method::POST,
            "/payment_intents",
            Some(&form_data),
        ).await?;

        Ok(CreateOrderResponse {
            order_id,
            gateway: PaymentGateway::Stripe,
            gateway_order_id: payment_intent.id,
            amount_cents: payment_intent.amount,
            currency: payment_intent.currency.to_uppercase(),
            razorpay_key_id: None,
            stripe_client_secret: Some(payment_intent.client_secret),
            stripe_publishable_key: Some(self.publishable_key.clone()),
        })
    }

    async fn verify_payment(&self, request: VerifyPaymentRequest) -> Result<PaymentVerification, AppError> {
        let payment_intent_id = request.stripe_payment_intent_id
            .ok_or_else(|| AppError::bad_request("stripe_payment_intent_id required"))?;

        let payment_intent: StripePaymentIntent = self.stripe_request(
            reqwest::Method::GET,
            &format!("/payment_intents/{}", payment_intent_id),
            None,
        ).await?;

        let success = payment_intent.status == "succeeded";

        // Get payment method details if available
        let payment_method = if let Some(pm_id) = &payment_intent.payment_method {
            let pm: Result<StripePaymentMethod, _> = self.stripe_request(
                reqwest::Method::GET,
                &format!("/payment_methods/{}", pm_id),
                None,
            ).await;

            pm.ok().map(|pm| {
                if let Some(card) = pm.card {
                    format!("{} ****{}", card.brand, card.last4)
                } else {
                    pm.payment_type
                }
            })
        } else {
            None
        };

        Ok(PaymentVerification {
            success,
            order_id: request.order_id,
            transaction_id: payment_intent.id,
            amount_cents: payment_intent.amount,
            currency: payment_intent.currency.to_uppercase(),
            payment_method,
            error_message: if !success {
                Some(format!("Payment status: {}", payment_intent.status))
            } else {
                None
            },
        })
    }

    async fn create_subscription(&self, request: CreateSubscriptionRequest) -> Result<SubscriptionResponse, AppError> {
        // In production, you'd look up the Stripe price_id from your products table
        let price_id = format!("price_{}", request.product_id);

        let payment_method = request.payment_method_id
            .ok_or_else(|| AppError::bad_request("payment_method_id required for Stripe subscriptions"))?;

        // First, create or get customer
        // For simplicity, we'll create a new subscription directly
        // In production, you'd manage Stripe customers

        let form_data = vec![
            ("items[0][price]", price_id.as_str()),
            ("default_payment_method", &payment_method),
            ("payment_behavior", "default_incomplete"),
            ("expand[]", "latest_invoice.payment_intent"),
        ];

        let subscription: StripeSubscription = self.stripe_request(
            reqwest::Method::POST,
            "/subscriptions",
            Some(&form_data),
        ).await?;

        Ok(SubscriptionResponse {
            subscription_id: format!("sub_{}", uuid::Uuid::new_v4()),
            gateway: PaymentGateway::Stripe,
            gateway_subscription_id: subscription.id.clone(),
            status: subscription.status,
            current_period_start: subscription.current_period_start,
            current_period_end: subscription.current_period_end,
            razorpay_subscription_id: None,
            stripe_subscription_id: Some(subscription.id),
        })
    }

    async fn cancel_subscription(&self, subscription_id: &str) -> Result<(), AppError> {
        let _: StripeSubscription = self.stripe_request(
            reqwest::Method::DELETE,
            &format!("/subscriptions/{}", subscription_id),
            None,
        ).await?;

        Ok(())
    }

    async fn create_refund(&self, payment_intent_id: &str, amount_cents: Option<i64>) -> Result<String, AppError> {
        let mut form_data = vec![
            ("payment_intent", payment_intent_id),
        ];

        let amount_str;
        if let Some(amount) = amount_cents {
            amount_str = amount.to_string();
            form_data.push(("amount", &amount_str));
        }

        let refund: StripeRefund = self.stripe_request(
            reqwest::Method::POST,
            "/refunds",
            Some(&form_data),
        ).await?;

        Ok(refund.id)
    }

    fn parse_webhook(&self, payload: &[u8], signature: &str) -> Result<WebhookEvent, AppError> {
        // Parse timestamp from signature header
        let parts: std::collections::HashMap<&str, &str> = signature
            .split(',')
            .filter_map(|part| {
                let mut split = part.split('=');
                Some((split.next()?, split.next()?))
            })
            .collect();

        let timestamp: i64 = parts.get("t")
            .and_then(|t| t.parse().ok())
            .ok_or_else(|| AppError::bad_request("Missing timestamp in signature"))?;

        // Verify signature
        if !self.verify_webhook_signature(payload, signature, timestamp) {
            return Err(AppError::unauthorized("Invalid webhook signature"));
        }

        let webhook: StripeWebhookEvent = serde_json::from_slice(payload)
            .map_err(|e| AppError::bad_request(format!("Invalid webhook payload: {}", e)))?;

        let event_type = match webhook.event_type.as_str() {
            "payment_intent.succeeded" => WebhookEventType::OrderPaid,
            "payment_intent.payment_failed" => WebhookEventType::OrderFailed,
            "customer.subscription.created" | "customer.subscription.updated" => {
                // Check if it's becoming active
                let status = webhook.data.object.get("status")
                    .and_then(|s| s.as_str())
                    .unwrap_or("");
                if status == "active" {
                    WebhookEventType::SubscriptionActivated
                } else {
                    WebhookEventType::SubscriptionCharged
                }
            }
            "customer.subscription.deleted" => WebhookEventType::SubscriptionCancelled,
            "invoice.paid" => WebhookEventType::SubscriptionCharged,
            "invoice.payment_failed" => WebhookEventType::SubscriptionExpired,
            "charge.refunded" => WebhookEventType::RefundCreated,
            "charge.dispute.created" => WebhookEventType::DisputeCreated,
            "charge.dispute.closed" => {
                let status = webhook.data.object.get("status")
                    .and_then(|s| s.as_str())
                    .unwrap_or("");
                if status == "won" {
                    WebhookEventType::DisputeWon
                } else {
                    WebhookEventType::DisputeLost
                }
            }
            _ => return Err(AppError::bad_request(format!("Unhandled event: {}", webhook.event_type))),
        };

        Ok(WebhookEvent {
            event_type,
            gateway: PaymentGateway::Stripe,
            gateway_event_id: webhook.id,
            data: webhook.data.object,
        })
    }
}
