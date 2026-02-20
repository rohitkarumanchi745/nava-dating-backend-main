//! Retry utilities with exponential backoff and jitter
//!
//! Implements best practices for payment retries:
//! - Exponential backoff with full jitter
//! - Dead letter queue for failed payments
//! - Idempotent retry handling

use rand::Rng;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::error::AppError;

/// Retry configuration
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    /// Base delay in milliseconds
    pub base_delay_ms: u64,
    /// Maximum delay in milliseconds (cap)
    pub max_delay_ms: u64,
    /// Multiplier for exponential backoff (default: 2.0)
    pub multiplier: f64,
    /// Whether to use full jitter (recommended) or decorrelated jitter
    pub use_full_jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay_ms: 1000,    // 1 second
            max_delay_ms: 30000,    // 30 seconds
            multiplier: 2.0,
            use_full_jitter: true,
        }
    }
}

impl RetryConfig {
    /// Create a config for payment operations (more aggressive retries)
    pub fn for_payments() -> Self {
        Self {
            max_attempts: 5,
            base_delay_ms: 500,     // 500ms
            max_delay_ms: 60000,    // 60 seconds
            multiplier: 2.0,
            use_full_jitter: true,
        }
    }

    /// Create a config for webhook processing
    pub fn for_webhooks() -> Self {
        Self {
            max_attempts: 3,
            base_delay_ms: 1000,
            max_delay_ms: 10000,
            multiplier: 2.0,
            use_full_jitter: true,
        }
    }

    /// Calculate delay for a given attempt with jitter
    /// Uses "Full Jitter" algorithm: sleep = random_between(0, min(cap, base * 2^attempt))
    /// See: https://aws.amazon.com/blogs/architecture/exponential-backoff-and-jitter/
    pub fn calculate_delay(&self, attempt: u32) -> Duration {
        let mut rng = rand::thread_rng();

        // Calculate exponential delay
        let exp_delay = self.base_delay_ms as f64 * self.multiplier.powi(attempt as i32);
        let capped_delay = exp_delay.min(self.max_delay_ms as f64);

        // Apply full jitter (random between 0 and calculated delay)
        let jittered_delay = if self.use_full_jitter {
            rng.gen_range(0.0..capped_delay)
        } else {
            // Decorrelated jitter alternative
            let prev_delay = if attempt == 0 {
                self.base_delay_ms as f64
            } else {
                self.base_delay_ms as f64 * self.multiplier.powi((attempt - 1) as i32)
            };
            rng.gen_range(self.base_delay_ms as f64..prev_delay * 3.0).min(capped_delay)
        };

        Duration::from_millis(jittered_delay as u64)
    }
}

/// Retry result with attempt tracking
#[derive(Debug)]
pub struct RetryResult<T> {
    pub value: T,
    pub attempts: u32,
    pub total_delay_ms: u64,
}

/// Execute a function with retry logic
pub async fn with_retry<F, Fut, T>(
    config: &RetryConfig,
    operation_name: &str,
    mut f: F,
) -> Result<RetryResult<T>, AppError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, AppError>>,
{
    let mut attempts = 0;
    let mut total_delay_ms = 0u64;
    let mut last_error: Option<AppError> = None;

    while attempts < config.max_attempts {
        match f().await {
            Ok(value) => {
                if attempts > 0 {
                    info!(
                        operation = operation_name,
                        attempts = attempts + 1,
                        "Operation succeeded after retries"
                    );
                }
                return Ok(RetryResult {
                    value,
                    attempts: attempts + 1,
                    total_delay_ms,
                });
            }
            Err(e) => {
                last_error = Some(e.clone());
                attempts += 1;

                if attempts >= config.max_attempts {
                    error!(
                        operation = operation_name,
                        attempts = attempts,
                        error = %e,
                        "Operation failed after all retries"
                    );
                    break;
                }

                // Check if error is retryable
                if !is_retryable_error(&e) {
                    warn!(
                        operation = operation_name,
                        error = %e,
                        "Non-retryable error, failing immediately"
                    );
                    return Err(e);
                }

                let delay = config.calculate_delay(attempts - 1);
                total_delay_ms += delay.as_millis() as u64;

                warn!(
                    operation = operation_name,
                    attempt = attempts,
                    delay_ms = delay.as_millis(),
                    error = %e,
                    "Retrying after error"
                );

                sleep(delay).await;
            }
        }
    }

    Err(last_error.unwrap_or_else(|| AppError::internal("Unknown retry error")))
}

/// Check if an error is retryable
fn is_retryable_error(error: &AppError) -> bool {
    // Check error message for retryable patterns
    let error_str = format!("{}", error);

    // Network/transient errors are retryable
    if error_str.contains("timeout")
        || error_str.contains("connection")
        || error_str.contains("temporarily unavailable")
        || error_str.contains("rate limit")
        || error_str.contains("503")
        || error_str.contains("502")
        || error_str.contains("504")
        || error_str.contains("network")
    {
        return true;
    }

    // Non-retryable: authentication, validation, not found errors
    if error_str.contains("unauthorized")
        || error_str.contains("forbidden")
        || error_str.contains("not found")
        || error_str.contains("invalid")
        || error_str.contains("bad request")
        || error_str.contains("duplicate")
    {
        return false;
    }

    // Default: retry on unknown errors (conservative approach for payments)
    true
}

/// Dead letter queue entry
#[derive(Debug, Clone)]
pub struct DeadLetterEntry {
    pub id: String,
    pub queue_name: String,
    pub payload: serde_json::Value,
    pub error_message: String,
    pub attempt_count: i32,
    pub first_failed_at: chrono::NaiveDateTime,
    pub last_failed_at: chrono::NaiveDateTime,
    pub status: DeadLetterStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeadLetterStatus {
    Pending,
    Retrying,
    Resolved,
    Abandoned,
}

impl DeadLetterStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            DeadLetterStatus::Pending => "pending",
            DeadLetterStatus::Retrying => "retrying",
            DeadLetterStatus::Resolved => "resolved",
            DeadLetterStatus::Abandoned => "abandoned",
        }
    }
}

/// Dead letter queue operations
pub struct DeadLetterQueue {
    db: sqlx::PgPool,
}

impl DeadLetterQueue {
    pub fn new(db: sqlx::PgPool) -> Self {
        Self { db }
    }

    /// Add a failed operation to the dead letter queue
    pub async fn enqueue(
        &self,
        queue_name: &str,
        payload: serde_json::Value,
        error_message: &str,
    ) -> Result<String, AppError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().naive_utc();

        sqlx::query(
            r#"
            INSERT INTO dead_letter_queue (id, queue_name, payload, error_message, attempt_count, first_failed_at, last_failed_at, status)
            VALUES ($1, $2, $3, $4, 1, $5, $5, 'pending')
            "#,
        )
        .bind(&id)
        .bind(queue_name)
        .bind(&payload)
        .bind(error_message)
        .bind(now)
        .execute(&self.db)
        .await?;

        warn!(
            dlq_id = %id,
            queue = queue_name,
            error = error_message,
            "Added entry to dead letter queue"
        );

        Ok(id)
    }

    /// Update an existing DLQ entry on retry failure
    pub async fn update_failure(
        &self,
        id: &str,
        error_message: &str,
    ) -> Result<(), AppError> {
        let now = chrono::Utc::now().naive_utc();

        sqlx::query(
            r#"
            UPDATE dead_letter_queue
            SET attempt_count = attempt_count + 1,
                last_failed_at = $2,
                error_message = $3,
                status = CASE WHEN attempt_count >= 10 THEN 'abandoned' ELSE 'pending' END
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(now)
        .bind(error_message)
        .execute(&self.db)
        .await?;

        Ok(())
    }

    /// Mark a DLQ entry as resolved
    pub async fn resolve(&self, id: &str) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE dead_letter_queue SET status = 'resolved', resolved_at = NOW() WHERE id = $1",
        )
        .bind(id)
        .execute(&self.db)
        .await?;

        info!(dlq_id = %id, "Dead letter queue entry resolved");
        Ok(())
    }

    /// Get pending entries for processing
    pub async fn get_pending(
        &self,
        queue_name: &str,
        limit: i64,
    ) -> Result<Vec<DeadLetterEntry>, AppError> {
        let rows = sqlx::query_as::<_, (String, String, serde_json::Value, String, i32, chrono::NaiveDateTime, chrono::NaiveDateTime, String)>(
            r#"
            SELECT id, queue_name, payload, error_message, attempt_count, first_failed_at, last_failed_at, status
            FROM dead_letter_queue
            WHERE queue_name = $1 AND status = 'pending'
            ORDER BY first_failed_at ASC
            LIMIT $2
            "#,
        )
        .bind(queue_name)
        .bind(limit)
        .fetch_all(&self.db)
        .await?;

        Ok(rows.into_iter().map(|(id, queue_name, payload, error_message, attempt_count, first_failed_at, last_failed_at, status)| {
            DeadLetterEntry {
                id,
                queue_name,
                payload,
                error_message,
                attempt_count,
                first_failed_at,
                last_failed_at,
                status: match status.as_str() {
                    "pending" => DeadLetterStatus::Pending,
                    "retrying" => DeadLetterStatus::Retrying,
                    "resolved" => DeadLetterStatus::Resolved,
                    _ => DeadLetterStatus::Abandoned,
                },
            }
        }).collect())
    }

    /// Get DLQ statistics
    pub async fn get_stats(&self) -> Result<serde_json::Value, AppError> {
        let stats = sqlx::query_as::<_, (String, String, i64)>(
            r#"
            SELECT queue_name, status, COUNT(*) as count
            FROM dead_letter_queue
            GROUP BY queue_name, status
            ORDER BY queue_name, status
            "#,
        )
        .fetch_all(&self.db)
        .await?;

        let mut result: std::collections::HashMap<String, std::collections::HashMap<String, i64>> =
            std::collections::HashMap::new();

        for (queue_name, status, count) in stats {
            result.entry(queue_name).or_default().insert(status, count);
        }

        Ok(serde_json::json!(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_delay_increases() {
        let config = RetryConfig::default();

        // With jitter, we can only test that delays are within expected bounds
        for attempt in 0..5 {
            let delay = config.calculate_delay(attempt);
            let max_expected = config.base_delay_ms as f64 * config.multiplier.powi(attempt as i32);
            let capped_max = max_expected.min(config.max_delay_ms as f64);

            assert!(delay.as_millis() <= capped_max as u128);
        }
    }

    #[test]
    fn test_retry_delay_capped() {
        let config = RetryConfig {
            max_delay_ms: 5000,
            ..Default::default()
        };

        // Even with high attempt count, delay should be capped
        let delay = config.calculate_delay(100);
        assert!(delay.as_millis() <= 5000);
    }

    #[test]
    fn test_retryable_errors() {
        let timeout_err = AppError::internal("Connection timeout");
        assert!(is_retryable_error(&timeout_err));

        let auth_err = AppError::unauthorized("Invalid token");
        assert!(!is_retryable_error(&auth_err));

        let not_found_err = AppError::not_found("Resource not found");
        assert!(!is_retryable_error(&not_found_err));
    }
}
