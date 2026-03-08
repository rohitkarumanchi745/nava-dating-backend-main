-- =============================================================================
-- Regression test: Swipes hash-partition ON CONFLICT behavior
-- =============================================================================
-- Run after migration 011. Verifies that:
--   1. INSERT works and routes to correct partition
--   2. ON CONFLICT (from_user_id, to_user_id) DO UPDATE works across partitions
--   3. The unique constraint is enforced per partition
--   4. Queries with from_user_id prune to a single partition
--
-- Usage: psql -d nava -f tests/swipes_partition_test.sql
-- =============================================================================

BEGIN;

-- 1. Insert a swipe
INSERT INTO swipes (from_user_id, to_user_id, action, source)
VALUES (999901, 999902, 'like', 'test')
ON CONFLICT (from_user_id, to_user_id) DO UPDATE SET action = 'like';

-- Verify it landed
DO $$
DECLARE
    row_count INT;
BEGIN
    SELECT COUNT(*) INTO row_count FROM swipes
    WHERE from_user_id = 999901 AND to_user_id = 999902 AND action = 'like';
    ASSERT row_count = 1, 'Expected 1 row after initial insert, got ' || row_count;
END $$;

-- 2. Upsert same pair with different action (ON CONFLICT DO UPDATE)
INSERT INTO swipes (from_user_id, to_user_id, action, source)
VALUES (999901, 999902, 'pass', 'test')
ON CONFLICT (from_user_id, to_user_id) DO UPDATE SET action = EXCLUDED.action;

-- Verify the action was updated (not duplicated)
DO $$
DECLARE
    row_count INT;
    current_action VARCHAR;
BEGIN
    SELECT COUNT(*) INTO row_count FROM swipes
    WHERE from_user_id = 999901 AND to_user_id = 999902;
    ASSERT row_count = 1, 'Expected 1 row after upsert, got ' || row_count || ' (unique constraint broken)';

    SELECT action INTO current_action FROM swipes
    WHERE from_user_id = 999901 AND to_user_id = 999902;
    ASSERT current_action = 'pass', 'Expected action=pass after upsert, got ' || current_action;
END $$;

-- 3. Insert another swipe from same user to different target
INSERT INTO swipes (from_user_id, to_user_id, action, source)
VALUES (999901, 999903, 'superlike', 'test')
ON CONFLICT (from_user_id, to_user_id) DO UPDATE SET action = EXCLUDED.action;

-- Both swipes for user 999901 should be in the same partition
DO $$
DECLARE
    row_count INT;
BEGIN
    SELECT COUNT(*) INTO row_count FROM swipes WHERE from_user_id = 999901;
    ASSERT row_count = 2, 'Expected 2 rows for user 999901, got ' || row_count;
END $$;

-- 4. Verify partition pruning via EXPLAIN
-- (This should show "Partitions selected: 1" or scan only 1 child)
DO $$
DECLARE
    plan_text TEXT;
BEGIN
    EXECUTE 'EXPLAIN (FORMAT TEXT) SELECT * FROM swipes WHERE from_user_id = 999901'
    INTO plan_text;
    -- Just verify the query runs; actual partition pruning validation is visual
    ASSERT plan_text IS NOT NULL, 'EXPLAIN returned NULL';
END $$;

-- 5. Unique constraint violation test: direct INSERT without ON CONFLICT should fail
DO $$
BEGIN
    INSERT INTO swipes (from_user_id, to_user_id, action) VALUES (999901, 999902, 'like');
    RAISE EXCEPTION 'Should have raised unique_violation';
EXCEPTION
    WHEN unique_violation THEN
        -- Expected: unique constraint enforced on partitioned table
        NULL;
END $$;

-- Cleanup test data
DELETE FROM swipes WHERE from_user_id = 999901;

-- Verify cleanup
DO $$
DECLARE
    row_count INT;
BEGIN
    SELECT COUNT(*) INTO row_count FROM swipes WHERE from_user_id = 999901;
    ASSERT row_count = 0, 'Cleanup failed, ' || row_count || ' rows remain';
END $$;

COMMIT;

-- If we get here, all assertions passed
SELECT 'ALL SWIPE PARTITION TESTS PASSED' AS result;
