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

use base64::Engine as _;
use reqwest::Client;
use serde::Deserialize;

use crate::error::AppError;

const VERIFY_PROD: &str = "https://buy.itunes.apple.com/verifyReceipt";
const VERIFY_SANDBOX: &str = "https://sandbox.itunes.apple.com/verifyReceipt";
const PROD_API: &str = "https://api.storekit.itunes.apple.com";
const SANDBOX_API: &str = "https://api.storekit-sandbox.itunes.apple.com";

/// App Store Server API credentials (from App Store Connect) for verifying
/// StoreKit 2 signed transactions (JWS). See `verify_signed_transaction`.
#[derive(Clone)]
pub struct AppStoreServerConfig {
    pub issuer_id: String,
    pub key_id: String,
    /// Contents of the App Store Connect `.p8` EC private key (PEM).
    pub private_key_pem: String,
    /// true = production App Store Server API; false = sandbox first.
    pub production: bool,
}

/// Client for Apple receipt verification.
pub struct AppleClient {
    client: Client,
    shared_secret: String,
    /// Expected app bundle id. If non-empty, receipts for any other bundle are
    /// rejected. Empty disables the check (useful in local testing).
    bundle_id: String,
    /// App Store Server API config for StoreKit 2 JWS verification (optional).
    server_api: Option<AppStoreServerConfig>,
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
    pub fn new(
        shared_secret: String,
        bundle_id: String,
        server_api: Option<AppStoreServerConfig>,
    ) -> Self {
        Self {
            client: Client::new(),
            shared_secret,
            bundle_id,
            server_api,
        }
    }

    /// Whether StoreKit 2 JWS verification (App Store Server API) is configured.
    pub fn has_server_api(&self) -> bool {
        self.server_api.is_some()
    }

    /// Verify a StoreKit 2 signed transaction (JWS, from `Transaction.jwsRepresentation`)
    /// via the App Store Server API. We read the transaction id from the client's
    /// (untrusted) JWS, then fetch the authoritative signed transaction from Apple
    /// over an authenticated channel — Apple's response is the trust anchor, so we
    /// never rely on locally validating the client's signature.
    ///
    /// NOTE: this path can't be exercised without real App Store Connect keys +
    /// a real transaction; validate against sandbox before trusting it in prod.
    pub async fn verify_signed_transaction(
        &self,
        jws: &str,
    ) -> Result<Vec<VerifiedTransaction>, AppError> {
        let cfg = self
            .server_api
            .as_ref()
            .ok_or_else(|| AppError::internal("App Store Server API not configured"))?;

        // transactionId from the client's JWS (untrusted — only tells us what to look up).
        let claimed = decode_jws_payload(jws)
            .ok_or_else(|| AppError::bad_request("Malformed signed transaction"))?;
        let txn_id = claimed
            .get("transactionId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::bad_request("No transactionId in signed transaction"))?;

        let bearer = bearer_jwt(cfg, &self.bundle_id)?;

        // Try the configured environment first, then the other on a 404 (App
        // Review uses sandbox; production users use production).
        let bases: [&str; 2] = if cfg.production {
            [PROD_API, SANDBOX_API]
        } else {
            [SANDBOX_API, PROD_API]
        };

        let mut last_status = 0u16;
        for base in bases {
            let url = format!("{}/inApps/v1/transactions/{}", base, txn_id);
            let resp = self
                .client
                .get(&url)
                .bearer_auth(&bearer)
                .send()
                .await
                .map_err(|e| AppError::internal(format!("App Store Server API request failed: {}", e)))?;

            if resp.status().is_success() {
                let body: SignedTxnResponse = resp
                    .json()
                    .await
                    .map_err(|e| AppError::internal(format!("App Store Server API parse failed: {}", e)))?;

                // Apple's authoritative signed transaction (trusted — authenticated API).
                let payload = decode_jws_payload(&body.signed_transaction_info)
                    .ok_or_else(|| AppError::internal("Malformed signed transaction from Apple"))?;

                if !self.bundle_id.is_empty()
                    && payload.get("bundleId").and_then(|v| v.as_str()) != Some(self.bundle_id.as_str())
                {
                    return Err(AppError::bad_request("Transaction bundle_id mismatch"));
                }

                let get = |k: &str| payload.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
                return Ok(vec![VerifiedTransaction {
                    transaction_id: get("transactionId"),
                    original_transaction_id: get("originalTransactionId"),
                    product_id: get("productId"),
                    expires_date_ms: payload.get("expiresDate").and_then(|v| v.as_i64()),
                }]);
            }

            last_status = resp.status().as_u16();
            if last_status != 404 {
                break;
            }
        }

        Err(AppError::bad_request(format!(
            "App Store Server API could not verify transaction (status {})",
            last_status
        )))
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

#[derive(Deserialize)]
struct SignedTxnResponse {
    #[serde(rename = "signedTransactionInfo")]
    signed_transaction_info: String,
}

/// Decode a JWS/JWT payload (the middle segment) WITHOUT verifying the signature.
/// Used to read the transaction id from the client's JWS and to read Apple's
/// authoritative response (which is trusted because it came from the authenticated API).
fn decode_jws_payload(jws: &str) -> Option<serde_json::Value> {
    let mut parts = jws.split('.');
    let _header = parts.next()?;
    let payload_b64 = parts.next()?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64)
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Build the ES256 bearer JWT the App Store Server API requires, signed with the
/// App Store Connect `.p8` private key.
fn bearer_jwt(cfg: &AppStoreServerConfig, bundle_id: &str) -> Result<String, AppError> {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

    #[derive(serde::Serialize)]
    struct Claims<'a> {
        iss: &'a str,
        iat: i64,
        exp: i64,
        aud: &'a str,
        bid: &'a str,
    }

    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        iss: &cfg.issuer_id,
        iat: now,
        exp: now + 1200, // 20 min (Apple allows up to ~60)
        aud: "appstoreconnect-v1",
        bid: bundle_id,
    };

    let mut header = Header::new(Algorithm::ES256);
    header.kid = Some(cfg.key_id.clone());

    let key = EncodingKey::from_ec_pem(cfg.private_key_pem.as_bytes())
        .map_err(|e| AppError::internal(format!("Invalid Apple private key: {}", e)))?;

    encode(&header, &claims, &key)
        .map_err(|e| AppError::internal(format!("Failed to sign App Store JWT: {}", e)))
}
