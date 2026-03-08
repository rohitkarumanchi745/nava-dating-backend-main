-- ============================================================================
-- Migration 011: Swipes Hash Partitioning
-- ============================================================================
-- Hash-partitions the swipes table by from_user_id (8 partitions).
--
-- Why hash instead of range/time?
--   - UNIQUE(from_user_id, to_user_id) must be enforced per-partition.
--   - Hash on from_user_id keeps each user's swipes in one partition,
--     so the unique constraint works and upserts (ON CONFLICT) work.
--   - 8 partitions gives good parallelism for vacuum/analyze without
--     too much overhead. PG can prune to 1 partition per user query.
--
-- Strategy: rename old → create partitioned → copy data → drop old.
-- FKs to users dropped (enforced at app layer, same as migration 010).
-- ============================================================================

-- Step 1: Rename the original table
ALTER TABLE swipes RENAME TO swipes_old;

-- Detach sequences so we can reuse them
ALTER SEQUENCE swipes_id_seq OWNED BY NONE;

-- Step 2: Create hash-partitioned table
-- Include from_user_id in PK so UNIQUE constraint works across partitions
CREATE TABLE swipes (
    id          BIGINT NOT NULL DEFAULT nextval('swipes_id_seq'),
    from_user_id BIGINT NOT NULL,
    to_user_id  BIGINT NOT NULL,
    action      VARCHAR(20) NOT NULL,
    source      VARCHAR(30) DEFAULT 'discover',
    created_at  TIMESTAMP DEFAULT NOW(),
    PRIMARY KEY (id, from_user_id),
    CONSTRAINT unique_swipe UNIQUE (from_user_id, to_user_id)
) PARTITION BY HASH (from_user_id);

-- Reattach sequence
ALTER SEQUENCE swipes_id_seq OWNED BY swipes.id;

-- Step 3: Create 8 hash partitions
CREATE TABLE swipes_p0 PARTITION OF swipes FOR VALUES WITH (MODULUS 8, REMAINDER 0);
CREATE TABLE swipes_p1 PARTITION OF swipes FOR VALUES WITH (MODULUS 8, REMAINDER 1);
CREATE TABLE swipes_p2 PARTITION OF swipes FOR VALUES WITH (MODULUS 8, REMAINDER 2);
CREATE TABLE swipes_p3 PARTITION OF swipes FOR VALUES WITH (MODULUS 8, REMAINDER 3);
CREATE TABLE swipes_p4 PARTITION OF swipes FOR VALUES WITH (MODULUS 8, REMAINDER 4);
CREATE TABLE swipes_p5 PARTITION OF swipes FOR VALUES WITH (MODULUS 8, REMAINDER 5);
CREATE TABLE swipes_p6 PARTITION OF swipes FOR VALUES WITH (MODULUS 8, REMAINDER 6);
CREATE TABLE swipes_p7 PARTITION OF swipes FOR VALUES WITH (MODULUS 8, REMAINDER 7);

-- Step 4: Indexes (auto-propagated to all partitions)
CREATE INDEX idx_swipes_from_user ON swipes (from_user_id, created_at DESC);
CREATE INDEX idx_swipes_to_user ON swipes (to_user_id, action);
CREATE INDEX idx_swipes_action ON swipes (action, created_at DESC);

-- Step 5: Copy data from old table
INSERT INTO swipes (id, from_user_id, to_user_id, action, source, created_at)
SELECT id, from_user_id, to_user_id, action, source, created_at
FROM swipes_old;

-- Step 6: Drop old table
DROP TABLE swipes_old;
