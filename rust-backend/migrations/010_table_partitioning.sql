-- ============================================================================
-- Migration 010: Table Partitioning
-- ============================================================================
-- Partitions the two heaviest append-only tables by time (weekly).
-- PG native range partitioning on created_at.
--
-- Tables partitioned:
--   1. interaction_events  — append-only event log, time-range (weekly)
--   2. messages            — append-only chat log, time-range (weekly)
--
-- NOT partitioned:
--   - swipes: upsert-heavy (ON CONFLICT), unique constraint on (from,to)
--     doesn't work across time partitions. Better as a regular table.
--   - matches: low cardinality, frequently joined, no benefit.
--
-- Strategy: rename old → create partitioned → copy data → drop old.
-- FKs to parent tables (users, matches) are dropped since PG partitioned
-- tables don't support FK references to non-partitioned parents when the
-- FK column isn't part of the partition key. Integrity enforced at app layer.
-- ============================================================================

-- ============================================================================
-- 1. interaction_events — partition by created_at (weekly)
-- ============================================================================

-- Rename the original table
ALTER TABLE interaction_events RENAME TO interaction_events_old;

-- Detach sequence from old table so we can reuse it
ALTER SEQUENCE interaction_events_id_seq OWNED BY NONE;

-- Create partitioned table (no FKs — enforced at app layer)
CREATE TABLE interaction_events (
    id          INTEGER NOT NULL DEFAULT nextval('interaction_events_id_seq'),
    user_id     INTEGER NOT NULL,
    target_user_id INTEGER NOT NULL,
    event_type  VARCHAR(50) NOT NULL,
    event_metadata JSON,
    slate_id    VARCHAR(64),
    rank        INTEGER,
    surface     VARCHAR(50),
    reward      DOUBLE PRECISION,
    delay_ms    INTEGER,
    created_at  TIMESTAMP NOT NULL DEFAULT NOW(),
    PRIMARY KEY (id, created_at)
) PARTITION BY RANGE (created_at);

-- Attach sequence to new table
ALTER SEQUENCE interaction_events_id_seq OWNED BY interaction_events.id;

-- Create partitions: catch-all for historical data + weekly going forward (14 weeks)
CREATE TABLE interaction_events_historical PARTITION OF interaction_events
    FOR VALUES FROM (MINVALUE) TO ('2026-03-09');
CREATE TABLE interaction_events_w2026_10 PARTITION OF interaction_events
    FOR VALUES FROM ('2026-03-09') TO ('2026-03-16');
CREATE TABLE interaction_events_w2026_11 PARTITION OF interaction_events
    FOR VALUES FROM ('2026-03-16') TO ('2026-03-23');
CREATE TABLE interaction_events_w2026_12 PARTITION OF interaction_events
    FOR VALUES FROM ('2026-03-23') TO ('2026-03-30');
CREATE TABLE interaction_events_w2026_13 PARTITION OF interaction_events
    FOR VALUES FROM ('2026-03-30') TO ('2026-04-06');
CREATE TABLE interaction_events_w2026_14 PARTITION OF interaction_events
    FOR VALUES FROM ('2026-04-06') TO ('2026-04-13');
CREATE TABLE interaction_events_w2026_15 PARTITION OF interaction_events
    FOR VALUES FROM ('2026-04-13') TO ('2026-04-20');
CREATE TABLE interaction_events_w2026_16 PARTITION OF interaction_events
    FOR VALUES FROM ('2026-04-20') TO ('2026-04-27');
CREATE TABLE interaction_events_w2026_17 PARTITION OF interaction_events
    FOR VALUES FROM ('2026-04-27') TO ('2026-05-04');
CREATE TABLE interaction_events_w2026_18 PARTITION OF interaction_events
    FOR VALUES FROM ('2026-05-04') TO ('2026-05-11');
CREATE TABLE interaction_events_w2026_19 PARTITION OF interaction_events
    FOR VALUES FROM ('2026-05-11') TO ('2026-05-18');
CREATE TABLE interaction_events_w2026_20 PARTITION OF interaction_events
    FOR VALUES FROM ('2026-05-18') TO ('2026-05-25');
CREATE TABLE interaction_events_w2026_21 PARTITION OF interaction_events
    FOR VALUES FROM ('2026-05-25') TO ('2026-06-01');
CREATE TABLE interaction_events_w2026_22 PARTITION OF interaction_events
    FOR VALUES FROM ('2026-06-01') TO ('2026-06-08');

-- Indexes on partitioned table (auto-propagated to child partitions)
CREATE INDEX idx_ie_user_created ON interaction_events (user_id, created_at);
CREATE INDEX idx_ie_target ON interaction_events (target_user_id);
CREATE INDEX idx_ie_type ON interaction_events (event_type);
CREATE INDEX idx_ie_slate ON interaction_events (slate_id) WHERE slate_id IS NOT NULL;
CREATE INDEX idx_ie_created ON interaction_events (created_at);

-- Copy existing data from old table
INSERT INTO interaction_events SELECT * FROM interaction_events_old;

-- Drop old table
DROP TABLE interaction_events_old;

-- ============================================================================
-- 2. messages — partition by created_at (weekly)
-- ============================================================================

ALTER TABLE messages RENAME TO messages_old;
ALTER SEQUENCE messages_id_seq OWNED BY NONE;

CREATE TABLE messages (
    id                INTEGER NOT NULL DEFAULT nextval('messages_id_seq'),
    match_id          VARCHAR(50) NOT NULL,
    sender_id         INTEGER NOT NULL,
    receiver_id       INTEGER NOT NULL,
    content           TEXT NOT NULL,
    message_type      VARCHAR(20),
    is_read           BOOLEAN,
    read_at           TIMESTAMP,
    is_deleted        BOOLEAN,
    is_flagged        BOOLEAN,
    moderation_status VARCHAR(20),
    created_at        TIMESTAMP NOT NULL DEFAULT NOW(),
    PRIMARY KEY (id, created_at)
) PARTITION BY RANGE (created_at);

ALTER SEQUENCE messages_id_seq OWNED BY messages.id;

-- Partitions
CREATE TABLE messages_historical PARTITION OF messages
    FOR VALUES FROM (MINVALUE) TO ('2026-03-09');
CREATE TABLE messages_w2026_10 PARTITION OF messages
    FOR VALUES FROM ('2026-03-09') TO ('2026-03-16');
CREATE TABLE messages_w2026_11 PARTITION OF messages
    FOR VALUES FROM ('2026-03-16') TO ('2026-03-23');
CREATE TABLE messages_w2026_12 PARTITION OF messages
    FOR VALUES FROM ('2026-03-23') TO ('2026-03-30');
CREATE TABLE messages_w2026_13 PARTITION OF messages
    FOR VALUES FROM ('2026-03-30') TO ('2026-04-06');
CREATE TABLE messages_w2026_14 PARTITION OF messages
    FOR VALUES FROM ('2026-04-06') TO ('2026-04-13');
CREATE TABLE messages_w2026_15 PARTITION OF messages
    FOR VALUES FROM ('2026-04-13') TO ('2026-04-20');
CREATE TABLE messages_w2026_16 PARTITION OF messages
    FOR VALUES FROM ('2026-04-20') TO ('2026-04-27');
CREATE TABLE messages_w2026_17 PARTITION OF messages
    FOR VALUES FROM ('2026-04-27') TO ('2026-05-04');
CREATE TABLE messages_w2026_18 PARTITION OF messages
    FOR VALUES FROM ('2026-05-04') TO ('2026-05-11');
CREATE TABLE messages_w2026_19 PARTITION OF messages
    FOR VALUES FROM ('2026-05-11') TO ('2026-05-18');
CREATE TABLE messages_w2026_20 PARTITION OF messages
    FOR VALUES FROM ('2026-05-18') TO ('2026-05-25');
CREATE TABLE messages_w2026_21 PARTITION OF messages
    FOR VALUES FROM ('2026-05-25') TO ('2026-06-01');
CREATE TABLE messages_w2026_22 PARTITION OF messages
    FOR VALUES FROM ('2026-06-01') TO ('2026-06-08');

-- Indexes
CREATE INDEX idx_msg_match_time ON messages (match_id, created_at);
CREATE INDEX idx_msg_receiver_read ON messages (receiver_id, is_read);
CREATE INDEX idx_msg_sender ON messages (sender_id);
CREATE INDEX idx_msg_created ON messages (created_at);
CREATE INDEX idx_msg_match ON messages (match_id);

-- Copy and drop
INSERT INTO messages SELECT * FROM messages_old;
DROP TABLE messages_old;

-- ============================================================================
-- Auto-partition management function
-- ============================================================================
-- Call weekly via pg_cron or a scheduled Rust job:
--   SELECT create_weekly_partitions('interaction_events', 4);
--   SELECT create_weekly_partitions('messages', 4);
-- ============================================================================

CREATE OR REPLACE FUNCTION create_weekly_partitions(
    p_table_name TEXT,
    p_weeks_ahead INTEGER DEFAULT 4
) RETURNS void AS $$
DECLARE
    start_date DATE;
    end_date DATE;
    partition_name TEXT;
    week_num INTEGER;
    i INTEGER;
BEGIN
    FOR i IN 0..p_weeks_ahead-1 LOOP
        start_date := date_trunc('week', CURRENT_DATE + (i * INTERVAL '1 week'));
        end_date := start_date + INTERVAL '7 days';
        week_num := EXTRACT(WEEK FROM start_date);
        partition_name := p_table_name || '_w' || EXTRACT(YEAR FROM start_date)::TEXT || '_' || LPAD(week_num::TEXT, 2, '0');

        -- Only create if it doesn't exist
        IF NOT EXISTS (
            SELECT 1 FROM pg_class WHERE relname = partition_name
        ) THEN
            EXECUTE format(
                'CREATE TABLE %I PARTITION OF %I FOR VALUES FROM (%L) TO (%L)',
                partition_name, p_table_name, start_date::TEXT, end_date::TEXT
            );
            RAISE NOTICE 'Created partition: %', partition_name;
        END IF;
    END LOOP;
END;
$$ LANGUAGE plpgsql;
