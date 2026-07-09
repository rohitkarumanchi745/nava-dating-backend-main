-- 032_lora_adapters.sql
-- Per-user LoRA adapter lifecycle for on-device personalized chat suggestions.
--
-- The phone contributes privacy-safe training signal (never raw chats) and
-- downloads the small adapter the server trains. Training itself runs in the
-- Python FedLoRA worker (scripts/fedlora_trainer.py), orchestrated through the
-- job table below.

-- Trained adapters, versioned per user. Exactly one 'active' row per user.
CREATE TABLE IF NOT EXISTS user_lora_adapters (
    id            SERIAL PRIMARY KEY,
    user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    version       INTEGER NOT NULL,
    storage_url   TEXT    NOT NULL,          -- CDN/S3 URL the device downloads
    sha256        TEXT    NOT NULL,          -- integrity check on the device
    size_bytes    BIGINT  NOT NULL DEFAULT 0,
    base_model    TEXT    NOT NULL DEFAULT 'bitnet-b1.58-2B-4T',
    rank          INTEGER NOT NULL DEFAULT 8,
    status        TEXT    NOT NULL DEFAULT 'active',  -- active | superseded | failed
    metrics       JSONB   NOT NULL DEFAULT '{}'::jsonb,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    activated_at  TIMESTAMPTZ,
    UNIQUE (user_id, version)
);

CREATE INDEX IF NOT EXISTS idx_lora_adapters_active
    ON user_lora_adapters(user_id, status) WHERE status = 'active';

-- Client-submitted federated training signal. This is aggregated LoRA-gradient
-- / preference signal + DP metadata, NOT raw messages.
CREATE TABLE IF NOT EXISTS lora_training_signals (
    id           SERIAL PRIMARY KEY,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    payload      JSONB   NOT NULL,           -- opaque to the API; the trainer reads it
    num_samples  INTEGER NOT NULL DEFAULT 0,
    dp_epsilon   DOUBLE PRECISION,
    dp_delta     DOUBLE PRECISION,
    consumed     BOOLEAN NOT NULL DEFAULT FALSE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_lora_signals_pending
    ON lora_training_signals(user_id) WHERE consumed = FALSE;

-- Training jobs the Python worker claims and executes.
CREATE TABLE IF NOT EXISTS lora_training_jobs (
    id           SERIAL PRIMARY KEY,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status       TEXT    NOT NULL DEFAULT 'pending',  -- pending | running | completed | failed
    round        INTEGER NOT NULL DEFAULT 0,
    error        TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at   TIMESTAMPTZ,
    finished_at  TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_lora_jobs_pending
    ON lora_training_jobs(created_at) WHERE status = 'pending';
-- At most one open (pending/running) job per user.
CREATE UNIQUE INDEX IF NOT EXISTS idx_lora_jobs_one_open_per_user
    ON lora_training_jobs(user_id) WHERE status IN ('pending', 'running');
