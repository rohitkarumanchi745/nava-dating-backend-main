//! Startup bootstrap + self-driving jobs for the AI features.
//!
//! - `ensure_ai_schema`: idempotently creates the LoRA / auto-match / GNN tables
//!   so the endpoints work regardless of the externally-managed migration flow.
//! - `produce_lora_signals`: server-side FedLoRA data producer — mines recent
//!   (incoming -> user reply) message pairs into SFT examples so the training
//!   loop has data even before the device submits any. Off by default.
//! - `enqueue_lora_training`: turns accumulated signal into training jobs.

use std::collections::HashMap;

use serde_json::json;
use sqlx::PgPool;
use tracing::warn;

/// Idempotent DDL for every AI feature table. Safe to run on every boot.
pub async fn ensure_ai_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    const STATEMENTS: &[&str] = &[
        // --- FedLoRA (032) ---
        "CREATE TABLE IF NOT EXISTS user_lora_adapters (
            id SERIAL PRIMARY KEY,
            user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            version INTEGER NOT NULL,
            storage_url TEXT NOT NULL,
            sha256 TEXT NOT NULL,
            size_bytes BIGINT NOT NULL DEFAULT 0,
            base_model TEXT NOT NULL DEFAULT 'bitnet-b1.58-2B-4T',
            rank INTEGER NOT NULL DEFAULT 8,
            status TEXT NOT NULL DEFAULT 'active',
            metrics JSONB NOT NULL DEFAULT '{}'::jsonb,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            activated_at TIMESTAMPTZ,
            UNIQUE (user_id, version))",
        "CREATE INDEX IF NOT EXISTS idx_lora_adapters_active ON user_lora_adapters(user_id, status) WHERE status = 'active'",
        "CREATE TABLE IF NOT EXISTS lora_training_signals (
            id SERIAL PRIMARY KEY,
            user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            payload JSONB NOT NULL,
            num_samples INTEGER NOT NULL DEFAULT 0,
            dp_epsilon DOUBLE PRECISION,
            dp_delta DOUBLE PRECISION,
            consumed BOOLEAN NOT NULL DEFAULT FALSE,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        "CREATE INDEX IF NOT EXISTS idx_lora_signals_pending ON lora_training_signals(user_id) WHERE consumed = FALSE",
        "CREATE TABLE IF NOT EXISTS lora_training_jobs (
            id SERIAL PRIMARY KEY,
            user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            status TEXT NOT NULL DEFAULT 'pending',
            round INTEGER NOT NULL DEFAULT 0,
            error TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            started_at TIMESTAMPTZ,
            finished_at TIMESTAMPTZ)",
        "CREATE INDEX IF NOT EXISTS idx_lora_jobs_pending ON lora_training_jobs(created_at) WHERE status = 'pending'",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_lora_jobs_one_open_per_user ON lora_training_jobs(user_id) WHERE status IN ('pending','running')",
        // --- Auto-match (033) ---
        "CREATE TABLE IF NOT EXISTS auto_match_suggestions (
            id SERIAL PRIMARY KEY,
            user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            candidate_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            mutual_score DOUBLE PRECISION NOT NULL,
            forward_score DOUBLE PRECISION,
            reverse_score DOUBLE PRECISION,
            status TEXT NOT NULL DEFAULT 'pending',
            match_id VARCHAR(36),
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            responded_at TIMESTAMPTZ)",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_auto_match_pending ON auto_match_suggestions(user_id, candidate_id) WHERE status = 'pending'",
        "CREATE INDEX IF NOT EXISTS idx_auto_match_user_status ON auto_match_suggestions(user_id, status, created_at DESC)",
        // --- GNN (034) ---
        "CREATE TABLE IF NOT EXISTS user_graph_embeddings (
            user_id BIGINT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
            embedding DOUBLE PRECISION[] NOT NULL,
            dim INTEGER NOT NULL,
            model_version INTEGER NOT NULL DEFAULT 1,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        "CREATE INDEX IF NOT EXISTS idx_gnn_emb_version ON user_graph_embeddings(model_version)",
        // --- Visual embeddings (035) ---
        "CREATE TABLE IF NOT EXISTS user_visual_embeddings (
            user_id BIGINT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
            embedding DOUBLE PRECISION[] NOT NULL,
            dim INTEGER NOT NULL,
            model_version INTEGER NOT NULL DEFAULT 1,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        "CREATE INDEX IF NOT EXISTS idx_visual_emb_version ON user_visual_embeddings(model_version)",
        // --- CoreML CLIP photo search (036) --- requires the pgvector extension
        "CREATE EXTENSION IF NOT EXISTS vector",
        "CREATE TABLE IF NOT EXISTS user_clip_embeddings (
            user_id BIGINT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
            embedding vector(512) NOT NULL,
            model_version TEXT NOT NULL DEFAULT 'mobileclip_s2',
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        "CREATE INDEX IF NOT EXISTS idx_clip_emb_hnsw ON user_clip_embeddings USING hnsw (embedding vector_cosine_ops)",
    ];

    // Per-statement error isolation: a failure (e.g. pgvector not installed)
    // logs and continues instead of blocking the other tables.
    for stmt in STATEMENTS {
        if let Err(e) = sqlx::query(stmt).execute(pool).await {
            warn!("ensure_ai_schema: statement skipped ({e})");
        }
    }
    Ok(())
}

/// Server-side FedLoRA signal producer. Mines recent (incoming message -> the
/// user's reply) pairs into SFT examples and stores one training signal per
/// user who has enough. Returns the number of users given a fresh signal.
pub async fn produce_lora_signals(pool: &PgPool) -> i64 {
    let rows = sqlx::query_as::<_, (i64, String, String)>(
        r#"
        WITH ctx AS (
            SELECT match_id, sender_id, content, created_at,
                   LAG(content)   OVER (PARTITION BY match_id ORDER BY created_at) AS prev_content,
                   LAG(sender_id) OVER (PARTITION BY match_id ORDER BY created_at) AS prev_sender
            FROM messages
            WHERE message_type = 'text' AND is_deleted = FALSE
              AND created_at > NOW() - INTERVAL '30 days'
        )
        SELECT sender_id, prev_content AS prompt, content AS completion
        FROM ctx
        WHERE prev_content IS NOT NULL AND prev_sender IS DISTINCT FROM sender_id
        ORDER BY sender_id
        LIMIT 20000
        "#,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut by_user: HashMap<i64, Vec<serde_json::Value>> = HashMap::new();
    for (uid, prompt, completion) in rows {
        let ex = by_user.entry(uid).or_default();
        if ex.len() < 50 {
            ex.push(json!({ "prompt": prompt, "completion": completion }));
        }
    }

    let mut produced = 0i64;
    for (uid, examples) in by_user {
        if examples.len() < 5 {
            continue;
        }
        // Skip users who already have unconsumed signal (no duplicates).
        let pending: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM lora_training_signals WHERE user_id = $1 AND consumed = FALSE",
        )
        .bind(uid)
        .fetch_one(pool)
        .await
        .unwrap_or(0);
        if pending > 0 {
            continue;
        }

        let num = examples.len() as i32;
        let payload = json!({ "examples": examples });
        let ok = sqlx::query(
            "INSERT INTO lora_training_signals (user_id, payload, num_samples) VALUES ($1,$2,$3)",
        )
        .bind(uid)
        .bind(&payload)
        .bind(num)
        .execute(pool)
        .await
        .is_ok();
        if ok {
            produced += 1;
        }
    }
    produced
}

/// Enqueue a training job for each user whose unconsumed signal has enough
/// samples and who has no open job. Returns the number of jobs enqueued.
pub async fn enqueue_lora_training(pool: &PgPool, min_samples: i64) -> i64 {
    sqlx::query_scalar::<_, i32>(
        r#"
        WITH eligible AS (
            SELECT user_id
            FROM lora_training_signals
            WHERE consumed = FALSE
            GROUP BY user_id
            HAVING SUM(COALESCE(num_samples, 0)) >= $1
        )
        INSERT INTO lora_training_jobs (user_id, status, round)
        SELECT e.user_id, 'pending',
               COALESCE((SELECT MAX(version) FROM user_lora_adapters a WHERE a.user_id = e.user_id), 0) + 1
        FROM eligible e
        ON CONFLICT DO NOTHING
        RETURNING id
        "#,
    )
    .bind(min_samples)
    .fetch_all(pool)
    .await
    .map(|v| v.len() as i64)
    .unwrap_or_else(|e| {
        warn!("enqueue_lora_training failed: {e}");
        0
    })
}
