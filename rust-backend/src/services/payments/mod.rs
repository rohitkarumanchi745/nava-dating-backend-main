//! Payment Gateway Integration Module
//!
//! Supports multiple payment providers with region-based routing:
//! - Razorpay (India) - 2% fees
//! - Stripe (Global) - 2.9% + 30¢ fees
//!
//! Web payments bypass 30% App Store fees!
//!
//! Features:
//! - Retry with exponential backoff and jitter
//! - Dead letter queue for failed payments
//! - Idempotent operations

pub mod apple;
pub mod razorpay;
pub mod retry;
pub mod stripe;

pub use retry::DeadLetterQueue;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::AppError;

/// Supported payment gateways
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaymentGateway {
    Razorpay,
    Stripe,
}

impl PaymentGateway {
    pub fn as_str(&self) -> &'static str {
        match self {
            PaymentGateway::Razorpay => "razorpay",
            PaymentGateway::Stripe => "stripe",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "razorpay" => Some(PaymentGateway::Razorpay),
            "stripe" => Some(PaymentGateway::Stripe),
            _ => None,
        }
    }

    /// Get recommended gateway for a region
    pub fn for_region(region_code: &str) -> Self {
        match region_code.to_uppercase().as_str() {
            "IN" => PaymentGateway::Razorpay,  // India - Razorpay preferred
            _ => PaymentGateway::Stripe,        // Rest of world - Stripe
        }
    }

    /// Get gateway fee percentage
    pub fn fee_percentage(&self) -> f64 {
        match self {
            PaymentGateway::Razorpay => 2.0,    // 2%
            PaymentGateway::Stripe => 2.9,      // 2.9% + fixed fee
        }
    }

    /// Get recommended gateway for a currency
    pub fn for_currency(currency: &Currency) -> Self {
        match currency {
            Currency::INR => PaymentGateway::Razorpay,
            _ => PaymentGateway::Stripe,
        }
    }
}

/// Currency codes with their smallest unit multiplier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Currency {
    INR,  // Indian Rupee (paise)
    USD,  // US Dollar (cents)
    EUR,  // Euro (cents)
    GBP,  // British Pound (pence)
    AED,  // UAE Dirham (fils)
    SGD,  // Singapore Dollar (cents)
    AUD,  // Australian Dollar (cents)
}

impl Currency {
    pub fn as_str(&self) -> &'static str {
        match self {
            Currency::INR => "INR",
            Currency::USD => "USD",
            Currency::EUR => "EUR",
            Currency::GBP => "GBP",
            Currency::AED => "AED",
            Currency::SGD => "SGD",
            Currency::AUD => "AUD",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "INR" => Some(Currency::INR),
            "USD" => Some(Currency::USD),
            "EUR" => Some(Currency::EUR),
            "GBP" => Some(Currency::GBP),
            "AED" => Some(Currency::AED),
            "SGD" => Some(Currency::SGD),
            "AUD" => Some(Currency::AUD),
            _ => None,
        }
    }

    /// Alias for from_str for backwards compatibility
    pub fn from_code(s: &str) -> Self {
        Self::from_str(s).unwrap_or(Currency::USD)
    }

    /// Smallest unit name
    pub fn smallest_unit(&self) -> &'static str {
        match self {
            Currency::INR => "paise",
            Currency::USD | Currency::EUR | Currency::SGD | Currency::AUD => "cents",
            Currency::GBP => "pence",
            Currency::AED => "fils",
        }
    }

    /// Symbol for display
    pub fn symbol(&self) -> &'static str {
        match self {
            Currency::INR => "₹",
            Currency::USD => "$",
            Currency::EUR => "€",
            Currency::GBP => "£",
            Currency::AED => "د.إ",
            Currency::SGD => "S$",
            Currency::AUD => "A$",
        }
    }

    /// Default currency for a region
    pub fn for_region(region_code: &str) -> Self {
        match region_code.to_uppercase().as_str() {
            "IN" => Currency::INR,
            "US" => Currency::USD,
            "GB" | "UK" => Currency::GBP,
            "AE" => Currency::AED,
            "SG" => Currency::SGD,
            "AU" => Currency::AUD,
            // EU countries
            "DE" | "FR" | "IT" | "ES" | "NL" | "BE" | "AT" | "IE" | "PT" | "FI" => Currency::EUR,
            _ => Currency::USD, // Default to USD
        }
    }
}

/// Create order request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOrderRequest {
    pub user_id: i32,
    pub product_id: String,
    pub amount_cents: i64,
    pub currency: Currency,
    pub region_code: String,
    pub platform: String,  // 'web', 'ios', 'android'
    pub metadata: Option<HashMap<String, String>>,
}

/// Create order response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOrderResponse {
    pub order_id: String,
    pub gateway: PaymentGateway,
    pub gateway_order_id: String,
    pub amount_cents: i64,
    pub currency: String,

    // Gateway-specific data for client
    #[serde(skip_serializing_if = "Option::is_none")]
    pub razorpay_key_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub stripe_client_secret: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub stripe_publishable_key: Option<String>,
}

/// Verify payment request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyPaymentRequest {
    pub order_id: String,
    pub gateway: PaymentGateway,

    // Razorpay fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub razorpay_payment_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub razorpay_signature: Option<String>,

    // Stripe fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stripe_payment_intent_id: Option<String>,
}

/// Payment verification result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentVerification {
    pub success: bool,
    pub order_id: String,
    pub transaction_id: String,
    pub amount_cents: i64,
    pub currency: String,
    pub payment_method: Option<String>,
    pub error_message: Option<String>,
}

/// Subscription creation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSubscriptionRequest {
    pub user_id: i32,
    pub product_id: String,
    pub region_code: String,
    pub payment_method_id: Option<String>,  // For Stripe
}

/// Subscription response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionResponse {
    pub subscription_id: String,
    pub gateway: PaymentGateway,
    pub gateway_subscription_id: String,
    pub status: String,
    pub current_period_start: i64,
    pub current_period_end: i64,

    // Client data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub razorpay_subscription_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stripe_subscription_id: Option<String>,
}

/// Webhook event types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookEventType {
    // Order events
    OrderPaid,
    OrderFailed,

    // Subscription events
    SubscriptionActivated,
    SubscriptionCharged,
    SubscriptionCancelled,
    SubscriptionExpired,

    // Refund events
    RefundCreated,
    RefundProcessed,

    // Dispute events
    DisputeCreated,
    DisputeWon,
    DisputeLost,
}

/// Parsed webhook event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEvent {
    pub event_type: WebhookEventType,
    pub gateway: PaymentGateway,
    pub gateway_event_id: String,
    pub data: serde_json::Value,
}

/// Payment gateway trait for abstraction
#[async_trait]
pub trait PaymentProvider: Send + Sync {
    /// Create a payment order
    async fn create_order(&self, request: CreateOrderRequest) -> Result<CreateOrderResponse, AppError>;

    /// Verify payment completion
    async fn verify_payment(&self, request: VerifyPaymentRequest) -> Result<PaymentVerification, AppError>;

    /// Create a subscription
    async fn create_subscription(&self, request: CreateSubscriptionRequest) -> Result<SubscriptionResponse, AppError>;

    /// Cancel a subscription
    async fn cancel_subscription(&self, subscription_id: &str) -> Result<(), AppError>;

    /// Process refund
    async fn create_refund(&self, transaction_id: &str, amount_cents: Option<i64>) -> Result<String, AppError>;

    /// Parse webhook payload
    fn parse_webhook(&self, payload: &[u8], signature: &str) -> Result<WebhookEvent, AppError>;
}

/// Multi-gateway payment service
pub struct PaymentService {
    razorpay: Option<razorpay::RazorpayClient>,
    stripe: Option<stripe::StripeClient>,
    apple: Option<apple::AppleClient>,
}

impl PaymentService {
    pub fn new(
        razorpay_key_id: Option<String>,
        razorpay_key_secret: Option<String>,
        stripe_secret_key: Option<String>,
        stripe_publishable_key: Option<String>,
        stripe_webhook_secret: Option<String>,
        apple_shared_secret: Option<String>,
        apple_bundle_id: Option<String>,
    ) -> Self {
        let razorpay = match (razorpay_key_id, razorpay_key_secret) {
            (Some(key_id), Some(key_secret)) => {
                Some(razorpay::RazorpayClient::new(key_id, key_secret))
            }
            _ => None,
        };

        let stripe = match (stripe_secret_key, stripe_publishable_key) {
            (Some(secret), Some(publishable)) => {
                Some(stripe::StripeClient::new(secret, publishable, stripe_webhook_secret))
            }
            _ => None,
        };

        // Apple only needs the shared secret to verify receipts. The bundle id
        // is an optional extra guard (defaults to empty = no bundle check).
        let apple = apple_shared_secret.map(|secret| {
            apple::AppleClient::new(secret, apple_bundle_id.unwrap_or_default())
        });

        Self { razorpay, stripe, apple }
    }

    /// Get the appropriate provider for a gateway
    fn get_provider(&self, gateway: PaymentGateway) -> Result<&dyn PaymentProvider, AppError> {
        match gateway {
            PaymentGateway::Razorpay => {
                self.razorpay.as_ref()
                    .map(|r| r as &dyn PaymentProvider)
                    .ok_or_else(|| AppError::internal("Razorpay not configured"))
            }
            PaymentGateway::Stripe => {
                self.stripe.as_ref()
                    .map(|s| s as &dyn PaymentProvider)
                    .ok_or_else(|| AppError::internal("Stripe not configured"))
            }
        }
    }

    /// Create order with automatic gateway selection
    pub async fn create_order(&self, request: CreateOrderRequest) -> Result<CreateOrderResponse, AppError> {
        let gateway = PaymentGateway::for_region(&request.region_code);
        let provider = self.get_provider(gateway)?;
        provider.create_order(request).await
    }

    /// Verify payment
    pub async fn verify_payment(&self, request: VerifyPaymentRequest) -> Result<PaymentVerification, AppError> {
        let provider = self.get_provider(request.gateway)?;
        provider.verify_payment(request).await
    }

    /// Create subscription
    pub async fn create_subscription(&self, request: CreateSubscriptionRequest) -> Result<SubscriptionResponse, AppError> {
        let gateway = PaymentGateway::for_region(&request.region_code);
        let provider = self.get_provider(gateway)?;
        provider.create_subscription(request).await
    }

    /// Cancel subscription
    pub async fn cancel_subscription(&self, gateway: PaymentGateway, subscription_id: &str) -> Result<(), AppError> {
        let provider = self.get_provider(gateway)?;
        provider.cancel_subscription(subscription_id).await
    }

    /// Process webhook
    pub fn process_webhook(&self, gateway: PaymentGateway, payload: &[u8], signature: &str) -> Result<WebhookEvent, AppError> {
        let provider = self.get_provider(gateway)?;
        provider.parse_webhook(payload, signature)
    }

    /// Check if Razorpay is available
    pub fn has_razorpay(&self) -> bool {
        self.razorpay.is_some()
    }

    /// Check if Stripe is available
    pub fn has_stripe(&self) -> bool {
        self.stripe.is_some()
    }

    /// Check if Apple receipt verification is configured
    pub fn has_apple(&self) -> bool {
        self.apple.is_some()
    }

    /// Verify an Apple App Store receipt server-side. Returns the transactions
    /// Apple considers valid, or an error if Apple is not configured or the
    /// receipt fails verification.
    pub async fn verify_apple_receipt(
        &self,
        receipt_b64: &str,
    ) -> Result<Vec<apple::VerifiedTransaction>, AppError> {
        let apple = self
            .apple
            .as_ref()
            .ok_or_else(|| AppError::internal("Apple receipt verification not configured"))?;
        apple.verify_receipt(receipt_b64).await
    }
}

/// Format amount for display
pub fn format_amount(amount_cents: i64, currency: Currency) -> String {
    let amount = amount_cents as f64 / 100.0;
    format!("{}{:.2}", currency.symbol(), amount)
}

/// Calculate gateway fee
pub fn calculate_fee(amount_cents: i64, gateway: PaymentGateway) -> i64 {
    let fee_rate = gateway.fee_percentage() / 100.0;
    let fixed_fee = match gateway {
        PaymentGateway::Stripe => 30, // 30 cents fixed fee
        PaymentGateway::Razorpay => 0,
    };
    ((amount_cents as f64 * fee_rate) as i64) + fixed_fee
}
