-- Dead letter queue for failed payment operations
-- Stores failed operations for manual review and retry

CREATE TABLE IF NOT EXISTS dead_letter_queue (
    id VARCHAR(36) PRIMARY KEY,
    queue_name VARCHAR(50) NOT NULL,
    payload JSONB NOT NULL,
    error_message TEXT NOT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 1,
    first_failed_at TIMESTAMP NOT NULL DEFAULT NOW(),
    last_failed_at TIMESTAMP NOT NULL DEFAULT NOW(),
    resolved_at TIMESTAMP,
    status VARCHAR(20) NOT NULL DEFAULT 'pending',
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),

    -- Ensure valid status
    CONSTRAINT valid_dlq_status CHECK (status IN ('pending', 'retrying', 'resolved', 'abandoned'))
);

-- Index for efficient queue processing
CREATE INDEX IF NOT EXISTS idx_dlq_queue_status ON dead_letter_queue(queue_name, status, first_failed_at);

-- Index for monitoring/reporting
CREATE INDEX IF NOT EXISTS idx_dlq_status ON dead_letter_queue(status, last_failed_at);

-- Table for tracking retry attempts with details
CREATE TABLE IF NOT EXISTS dead_letter_retry_log (
    id BIGSERIAL PRIMARY KEY,
    dlq_id VARCHAR(36) NOT NULL REFERENCES dead_letter_queue(id) ON DELETE CASCADE,
    attempt_number INTEGER NOT NULL,
    attempted_at TIMESTAMP NOT NULL DEFAULT NOW(),
    result VARCHAR(20) NOT NULL, -- 'success', 'failure'
    error_message TEXT,
    duration_ms INTEGER
);

CREATE INDEX IF NOT EXISTS idx_dlq_retry_log ON dead_letter_retry_log(dlq_id, attempted_at);

-- Payment retry tracking table for idempotent retries
CREATE TABLE IF NOT EXISTS payment_retry_tracking (
    id BIGSERIAL PRIMARY KEY,
    payment_order_id VARCHAR(36) NOT NULL,
    retry_key VARCHAR(255) NOT NULL,
    attempt_number INTEGER NOT NULL DEFAULT 1,
    status VARCHAR(20) NOT NULL DEFAULT 'pending',
    last_error TEXT,
    next_retry_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),

    CONSTRAINT valid_retry_status CHECK (status IN ('pending', 'in_progress', 'succeeded', 'failed', 'abandoned'))
);

-- Unique constraint for idempotent retry tracking
CREATE UNIQUE INDEX IF NOT EXISTS idx_payment_retry_key ON payment_retry_tracking(payment_order_id, retry_key);

-- Index for finding payments due for retry
CREATE INDEX IF NOT EXISTS idx_payment_retry_next ON payment_retry_tracking(status, next_retry_at)
WHERE status IN ('pending', 'failed');
