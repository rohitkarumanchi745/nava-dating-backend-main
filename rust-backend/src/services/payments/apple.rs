//! Apple App Store receipt verification.
//!
//! Verifies purchases server-side so a client cannot grant itself a product by
//! POSTing an arbitrary transaction id. The receipt blob is signed by Apple and
//! we verify it against Apple's `verifyReceipt` endpoint — Apple's response over
//! TLS is the trust anchor. We never decode or trust the receipt locally.
//!
//! `verifyReceipt` is Apple's classic endpoint; it still works for any build
//! that ships an App Store receipt. A pure StoreKit 2 pipeline can later switch
//! to the App Store Server API (JWS transaction verification) without changing
//! the handler contract: verify -> match the claimed transaction -> grant.

use reqwest::Client;
use serde::Deserialize;

use crate::error::AppError;

const VERIFY_PROD: &str = "https://buy.itunes.apple.com/verifyReceipt";
const VERIFY_SANDBOX: &str = "https://sandbox.itunes.apple.com/verifyReceipt";

/// Client for Apple receipt verification.
pub struct AppleClient {
    client: Client,
    shared_secret: String,
    /// Expected app bundle id. If non-empty, receipts for any other bundle are
    /// rejected. Empty disables the check (useful in local testing).
    bundle_id: String,
}

/// A single transaction Apple considers valid inside a verified receipt.
#[derive(Debug, Clone)]
pub struct VerifiedTransaction {
    pub transaction_id: String,
    pub original_transaction_id: String,
    pub product_id: String,
    /// Subscription expiry in epoch milliseconds, if this is a subscription.
    pub expires_date_ms: Option<i64>,
}

#[derive(Deserialize)]
struct AppleResponse {
    status: i64,
    #[serde(default)]
    receipt: Option<AppleReceipt>,
    /// Present for auto-renewable subscriptions.
    #[serde(default)]
    latest_receipt_info: Option<Vec<AppleInApp>>,
}

#[derive(Deserialize)]
struct AppleReceipt {
    #[serde(default)]
    bundle_id: Option<String>,
    #[serde(default)]
    in_app: Option<Vec<AppleInApp>>,
}

#[derive(Deserialize)]
struct AppleInApp {
    #[serde(default)]
    transaction_id: Option<String>,
    #[serde(default)]
    original_transaction_id: Option<String>,
    #[serde(default)]
    product_id: Option<String>,
    #[serde(default)]
    expires_date_ms: Option<String>,
}

impl AppleClient {
    pub fn new(shared_secret: String, bundle_id: String) -> Self {
        Self {
            client: Client::new(),
            shared_secret,
            bundle_id,
        }
    }

    /// Verify a base64-encoded App Store receipt against Apple. Returns every
    /// transaction Apple reports as valid within that receipt.
    ///
    /// Handles the sandbox/production split per Apple's guidance: always POST to
    /// production first; status `21007` means the receipt came from the sandbox,
    /// so retry against the sandbox endpoint. This lets the same production build
    /// pass App Review (which tests against sandbox).
    pub async fn verify_receipt(
        &self,
        receipt_b64: &str,
    ) -> Result<Vec<VerifiedTransaction>, AppError> {
        let first = self.post(VERIFY_PROD, receipt_b64).await?;
        let resp = if first.status == 21007 {
            self.post(VERIFY_SANDBOX, receipt_b64).await?
        } else {
            first
        };

        if resp.status != 0 {
            return Err(AppError::bad_request(format!(
                "Apple receipt verification failed (status {})",
                resp.status
            )));
        }

        let AppleResponse {
            receipt,
            latest_receipt_info,
            ..
        } = resp;

        // Reject receipts belonging to a different app.
        if !self.bundle_id.is_empty() {
            let bundle = receipt.as_ref().and_then(|r| r.bundle_id.as_deref());
            if bundle != Some(self.bundle_id.as_str()) {
                return Err(AppError::bad_request("Receipt bundle_id mismatch"));
            }
        }

        // Subscriptions surface in `latest_receipt_info`; one-off purchases in
        // `receipt.in_app`.
        let items = latest_receipt_info
            .or_else(|| receipt.and_then(|r| r.in_app))
            .unwrap_or_default();

        let txns = items
            .into_iter()
            .filter_map(|it| {
                Some(VerifiedTransaction {
                    transaction_id: it.transaction_id?,
                    original_transaction_id: it.original_transaction_id.unwrap_or_default(),
                    product_id: it.product_id.unwrap_or_default(),
                    expires_date_ms: it.expires_date_ms.and_then(|s| s.parse().ok()),
                })
            })
            .collect();

        Ok(txns)
    }

    async fn post(&self, url: &str, receipt_b64: &str) -> Result<AppleResponse, AppError> {
        let body = serde_json::json!({
            "receipt-data": receipt_b64,
            "password": self.shared_secret,
            "exclude-old-transactions": true,
        });

        let resp = self
            .client
            .post(url)
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::internal(format!("Apple verifyReceipt request failed: {}", e)))?;

        resp.json::<AppleResponse>()
            .await
            .map_err(|e| AppError::internal(format!("Apple verifyReceipt parse failed: {}", e)))
    }
}
