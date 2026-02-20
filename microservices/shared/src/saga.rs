//! Saga pattern for distributed transactions
//!
//! Implements the saga orchestration pattern for managing long-running
//! transactions across multiple microservices with compensation logic.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::RwLock;
use tracing::{error, info, warn};
use uuid::Uuid;

/// Saga step status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Compensating,
    Compensated,
}

/// Overall saga status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SagaStatus {
    Running,
    Completed,
    Failed,
    Compensating,
    Compensated,
}

/// Saga step definition
pub struct SagaStep<C> {
    pub name: String,
    pub execute: Box<
        dyn Fn(C) -> Pin<Box<dyn Future<Output = Result<C, SagaError>> + Send>> + Send + Sync,
    >,
    pub compensate: Box<
        dyn Fn(C) -> Pin<Box<dyn Future<Output = Result<(), SagaError>> + Send>> + Send + Sync,
    >,
}

/// Saga execution context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SagaContext {
    pub saga_id: Uuid,
    pub correlation_id: Uuid,
    pub data: serde_json::Value,
    pub step_results: HashMap<String, serde_json::Value>,
}

impl SagaContext {
    pub fn new(data: serde_json::Value) -> Self {
        Self {
            saga_id: Uuid::new_v4(),
            correlation_id: Uuid::new_v4(),
            data,
            step_results: HashMap::new(),
        }
    }

    pub fn with_correlation_id(mut self, correlation_id: Uuid) -> Self {
        self.correlation_id = correlation_id;
        self
    }

    pub fn set_result(&mut self, step_name: &str, result: serde_json::Value) {
        self.step_results.insert(step_name.to_string(), result);
    }

    pub fn get_result(&self, step_name: &str) -> Option<&serde_json::Value> {
        self.step_results.get(step_name)
    }
}

/// Saga execution record for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SagaRecord {
    pub saga_id: Uuid,
    pub saga_type: String,
    pub correlation_id: Uuid,
    pub status: SagaStatus,
    pub current_step: i32,
    pub context: serde_json::Value,
    pub steps: Vec<StepRecord>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepRecord {
    pub name: String,
    pub status: StepStatus,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
}

/// Saga error types
#[derive(Debug, thiserror::Error)]
pub enum SagaError {
    #[error("Step execution failed: {0}")]
    StepFailed(String),

    #[error("Compensation failed: {0}")]
    CompensationFailed(String),

    #[error("Saga not found: {0}")]
    NotFound(Uuid),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Timeout after {0} seconds")]
    Timeout(u64),
}

/// Saga orchestrator for managing distributed transactions
pub struct SagaOrchestrator {
    pool: PgPool,
    sagas: RwLock<HashMap<String, Arc<SagaDefinition>>>,
}

/// Definition of a saga with its steps
pub struct SagaDefinition {
    pub name: String,
    pub steps: Vec<SagaStepDef>,
    pub timeout_secs: u64,
}

pub struct SagaStepDef {
    pub name: String,
    pub service: String,
    pub action: String,
    pub compensation_action: String,
}

impl SagaOrchestrator {
    /// Create a new saga orchestrator
    pub async fn new(pool: PgPool) -> Result<Self, SagaError> {
        let orchestrator = Self {
            pool,
            sagas: RwLock::new(HashMap::new()),
        };

        orchestrator.ensure_tables().await?;
        Ok(orchestrator)
    }

    /// Ensure saga tables exist
    async fn ensure_tables(&self) -> Result<(), SagaError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS sagas (
                saga_id UUID PRIMARY KEY,
                saga_type VARCHAR(255) NOT NULL,
                correlation_id UUID NOT NULL,
                status VARCHAR(50) NOT NULL,
                current_step INTEGER NOT NULL DEFAULT 0,
                context JSONB NOT NULL,
                error_message TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS saga_steps (
                id BIGSERIAL PRIMARY KEY,
                saga_id UUID NOT NULL REFERENCES sagas(saga_id),
                step_index INTEGER NOT NULL,
                step_name VARCHAR(255) NOT NULL,
                status VARCHAR(50) NOT NULL,
                started_at TIMESTAMPTZ,
                completed_at TIMESTAMPTZ,
                error_message TEXT,
                UNIQUE(saga_id, step_index)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_sagas_status
            ON sagas (status) WHERE status IN ('running', 'compensating')
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Register a saga definition
    pub async fn register(&self, definition: SagaDefinition) {
        let mut sagas = self.sagas.write().await;
        sagas.insert(definition.name.clone(), Arc::new(definition));
    }

    /// Start a new saga
    pub async fn start(
        &self,
        saga_type: &str,
        context: SagaContext,
    ) -> Result<Uuid, SagaError> {
        let sagas = self.sagas.read().await;
        let definition = sagas
            .get(saga_type)
            .ok_or_else(|| SagaError::NotFound(context.saga_id))?;

        let saga_id = context.saga_id;

        // Create saga record
        sqlx::query(
            r#"
            INSERT INTO sagas (saga_id, saga_type, correlation_id, status, current_step, context)
            VALUES ($1, $2, $3, 'running', 0, $4)
            "#,
        )
        .bind(saga_id)
        .bind(saga_type)
        .bind(context.correlation_id)
        .bind(serde_json::to_value(&context)?)
        .execute(&self.pool)
        .await?;

        // Create step records
        for (i, step) in definition.steps.iter().enumerate() {
            sqlx::query(
                r#"
                INSERT INTO saga_steps (saga_id, step_index, step_name, status)
                VALUES ($1, $2, $3, 'pending')
                "#,
            )
            .bind(saga_id)
            .bind(i as i32)
            .bind(&step.name)
            .execute(&self.pool)
            .await?;
        }

        info!(saga_id = %saga_id, saga_type = %saga_type, "Saga started");

        Ok(saga_id)
    }

    /// Execute the next step of a saga
    pub async fn execute_step(
        &self,
        saga_id: Uuid,
        step_result: Result<serde_json::Value, String>,
    ) -> Result<bool, SagaError> {
        // Get current saga state
        let (saga_type, current_step, mut context): (String, i32, SagaContext) = sqlx::query_as(
            r#"
            SELECT saga_type, current_step, context
            FROM sagas
            WHERE saga_id = $1 AND status = 'running'
            "#,
        )
        .bind(saga_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(SagaError::NotFound(saga_id))?;

        let sagas = self.sagas.read().await;
        let definition = sagas
            .get(&saga_type)
            .ok_or(SagaError::NotFound(saga_id))?;

        match step_result {
            Ok(result) => {
                // Mark current step as completed
                sqlx::query(
                    r#"
                    UPDATE saga_steps
                    SET status = 'completed', completed_at = NOW()
                    WHERE saga_id = $1 AND step_index = $2
                    "#,
                )
                .bind(saga_id)
                .bind(current_step)
                .execute(&self.pool)
                .await?;

                // Store step result
                let step_name = &definition.steps[current_step as usize].name;
                context.set_result(step_name, result);

                let next_step = current_step + 1;

                if next_step as usize >= definition.steps.len() {
                    // Saga completed successfully
                    sqlx::query(
                        r#"
                        UPDATE sagas
                        SET status = 'completed', current_step = $2, context = $3, updated_at = NOW()
                        WHERE saga_id = $1
                        "#,
                    )
                    .bind(saga_id)
                    .bind(next_step)
                    .bind(serde_json::to_value(&context)?)
                    .execute(&self.pool)
                    .await?;

                    info!(saga_id = %saga_id, "Saga completed successfully");
                    return Ok(true);
                }

                // Move to next step
                sqlx::query(
                    r#"
                    UPDATE sagas
                    SET current_step = $2, context = $3, updated_at = NOW()
                    WHERE saga_id = $1
                    "#,
                )
                .bind(saga_id)
                .bind(next_step)
                .bind(serde_json::to_value(&context)?)
                .execute(&self.pool)
                .await?;

                // Mark next step as running
                sqlx::query(
                    r#"
                    UPDATE saga_steps
                    SET status = 'running', started_at = NOW()
                    WHERE saga_id = $1 AND step_index = $2
                    "#,
                )
                .bind(saga_id)
                .bind(next_step)
                .execute(&self.pool)
                .await?;

                info!(
                    saga_id = %saga_id,
                    step = next_step,
                    "Saga proceeding to next step"
                );

                Ok(false)
            }
            Err(error) => {
                // Mark step as failed
                sqlx::query(
                    r#"
                    UPDATE saga_steps
                    SET status = 'failed', completed_at = NOW(), error_message = $3
                    WHERE saga_id = $1 AND step_index = $2
                    "#,
                )
                .bind(saga_id)
                .bind(current_step)
                .bind(&error)
                .execute(&self.pool)
                .await?;

                // Start compensation
                sqlx::query(
                    r#"
                    UPDATE sagas
                    SET status = 'compensating', error_message = $2, updated_at = NOW()
                    WHERE saga_id = $1
                    "#,
                )
                .bind(saga_id)
                .bind(&error)
                .execute(&self.pool)
                .await?;

                warn!(
                    saga_id = %saga_id,
                    step = current_step,
                    error = %error,
                    "Saga step failed, starting compensation"
                );

                Ok(false)
            }
        }
    }

    /// Execute compensation for a failed saga
    pub async fn compensate_step(
        &self,
        saga_id: Uuid,
        step_index: i32,
        result: Result<(), String>,
    ) -> Result<bool, SagaError> {
        match result {
            Ok(()) => {
                // Mark step as compensated
                sqlx::query(
                    r#"
                    UPDATE saga_steps
                    SET status = 'compensated', completed_at = NOW()
                    WHERE saga_id = $1 AND step_index = $2
                    "#,
                )
                .bind(saga_id)
                .bind(step_index)
                .execute(&self.pool)
                .await?;

                if step_index == 0 {
                    // All compensations complete
                    sqlx::query(
                        r#"
                        UPDATE sagas
                        SET status = 'compensated', updated_at = NOW()
                        WHERE saga_id = $1
                        "#,
                    )
                    .bind(saga_id)
                    .execute(&self.pool)
                    .await?;

                    info!(saga_id = %saga_id, "Saga fully compensated");
                    return Ok(true);
                }

                // Continue compensating previous step
                sqlx::query(
                    r#"
                    UPDATE saga_steps
                    SET status = 'compensating', started_at = NOW()
                    WHERE saga_id = $1 AND step_index = $2
                    "#,
                )
                .bind(saga_id)
                .bind(step_index - 1)
                .execute(&self.pool)
                .await?;

                Ok(false)
            }
            Err(error) => {
                // Compensation failed - requires manual intervention
                sqlx::query(
                    r#"
                    UPDATE sagas
                    SET status = 'failed', error_message = $2, updated_at = NOW()
                    WHERE saga_id = $1
                    "#,
                )
                .bind(saga_id)
                .bind(format!("Compensation failed: {}", error))
                .execute(&self.pool)
                .await?;

                error!(
                    saga_id = %saga_id,
                    step = step_index,
                    error = %error,
                    "Saga compensation failed - manual intervention required"
                );

                Err(SagaError::CompensationFailed(error))
            }
        }
    }

    /// Get saga status
    pub async fn get_status(&self, saga_id: Uuid) -> Result<SagaRecord, SagaError> {
        let (saga_type, correlation_id, status, current_step, context, error_message, created_at, updated_at): (
            String, Uuid, String, i32, serde_json::Value, Option<String>, DateTime<Utc>, DateTime<Utc>
        ) = sqlx::query_as(
            r#"
            SELECT saga_type, correlation_id, status, current_step, context, error_message, created_at, updated_at
            FROM sagas
            WHERE saga_id = $1
            "#,
        )
        .bind(saga_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(SagaError::NotFound(saga_id))?;

        let steps: Vec<(String, String, Option<DateTime<Utc>>, Option<DateTime<Utc>>, Option<String>)> = sqlx::query_as(
            r#"
            SELECT step_name, status, started_at, completed_at, error_message
            FROM saga_steps
            WHERE saga_id = $1
            ORDER BY step_index
            "#,
        )
        .bind(saga_id)
        .fetch_all(&self.pool)
        .await?;

        let status = match status.as_str() {
            "running" => SagaStatus::Running,
            "completed" => SagaStatus::Completed,
            "failed" => SagaStatus::Failed,
            "compensating" => SagaStatus::Compensating,
            "compensated" => SagaStatus::Compensated,
            _ => SagaStatus::Failed,
        };

        let step_records: Vec<StepRecord> = steps
            .into_iter()
            .map(|(name, status, started_at, completed_at, error_message)| {
                let status = match status.as_str() {
                    "pending" => StepStatus::Pending,
                    "running" => StepStatus::Running,
                    "completed" => StepStatus::Completed,
                    "failed" => StepStatus::Failed,
                    "compensating" => StepStatus::Compensating,
                    "compensated" => StepStatus::Compensated,
                    _ => StepStatus::Pending,
                };
                StepRecord {
                    name,
                    status,
                    started_at,
                    completed_at,
                    error_message,
                }
            })
            .collect();

        Ok(SagaRecord {
            saga_id,
            saga_type,
            correlation_id,
            status,
            current_step,
            context,
            steps: step_records,
            created_at,
            updated_at,
            error_message,
        })
    }

    /// Recover incomplete sagas on startup
    pub async fn recover_incomplete(&self) -> Result<Vec<Uuid>, SagaError> {
        let incomplete: Vec<(Uuid,)> = sqlx::query_as(
            r#"
            SELECT saga_id
            FROM sagas
            WHERE status IN ('running', 'compensating')
              AND updated_at < NOW() - INTERVAL '5 minutes'
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let saga_ids: Vec<Uuid> = incomplete.into_iter().map(|(id,)| id).collect();

        if !saga_ids.is_empty() {
            warn!(count = saga_ids.len(), "Found incomplete sagas to recover");
        }

        Ok(saga_ids)
    }
}

// ============================================================================
// Example: Payment Saga
// ============================================================================

/// Example payment saga steps
pub fn create_payment_saga() -> SagaDefinition {
    SagaDefinition {
        name: "payment_subscription".to_string(),
        steps: vec![
            SagaStepDef {
                name: "create_order".to_string(),
                service: "payment-service".to_string(),
                action: "create_order".to_string(),
                compensation_action: "cancel_order".to_string(),
            },
            SagaStepDef {
                name: "process_payment".to_string(),
                service: "payment-service".to_string(),
                action: "process_payment".to_string(),
                compensation_action: "refund_payment".to_string(),
            },
            SagaStepDef {
                name: "activate_subscription".to_string(),
                service: "user-service".to_string(),
                action: "activate_premium".to_string(),
                compensation_action: "deactivate_premium".to_string(),
            },
            SagaStepDef {
                name: "send_confirmation".to_string(),
                service: "notification-service".to_string(),
                action: "send_payment_confirmation".to_string(),
                compensation_action: "noop".to_string(), // No compensation needed
            },
        ],
        timeout_secs: 300, // 5 minutes
    }
}
