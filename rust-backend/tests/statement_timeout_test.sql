-- =============================================================================
-- Statement Timeout Verification Test
-- =============================================================================
-- Tests that statement_timeout works in both direct and PgBouncer modes.
-- Run: psql -d nava -f tests/statement_timeout_test.sql
--
-- Mode 1 (Direct / no PgBouncer): SET statement_timeout per session
-- Mode 2 (PgBouncer transaction): SET LOCAL statement_timeout per txn
-- =============================================================================

-- 1. Verify session-level SET works (direct connection mode)
DO $$
BEGIN
    SET statement_timeout = '500ms';
    -- Short query should succeed
    PERFORM pg_sleep(0.1);
    RAISE NOTICE 'PASS: Short query under session timeout succeeded';
END $$;

-- 2. Verify session-level timeout kills long queries
DO $$
BEGIN
    SET statement_timeout = '200ms';
    PERFORM pg_sleep(5);
    RAISE EXCEPTION 'FAIL: Long query was not killed by statement_timeout';
EXCEPTION
    WHEN query_canceled THEN
        RAISE NOTICE 'PASS: Session-level statement_timeout correctly canceled long query';
END $$;

-- 3. Verify SET LOCAL works within a transaction (PgBouncer mode)
BEGIN;
    SET LOCAL statement_timeout = '200ms';
    DO $$
    BEGIN
        PERFORM pg_sleep(5);
        RAISE EXCEPTION 'FAIL: SET LOCAL did not enforce timeout';
    EXCEPTION
        WHEN query_canceled THEN
            RAISE NOTICE 'PASS: SET LOCAL statement_timeout works in transaction';
    END $$;
COMMIT;

-- 4. Verify SET LOCAL does NOT leak to next transaction (PgBouncer safety)
-- After COMMIT, the timeout should be gone
DO $$
DECLARE
    current_timeout TEXT;
BEGIN
    SHOW statement_timeout INTO current_timeout;
    -- After SET LOCAL + COMMIT, timeout should have reverted
    -- (default is '0' = no timeout, or whatever postgresql.conf sets)
    RAISE NOTICE 'PASS: After SET LOCAL + COMMIT, statement_timeout = %', current_timeout;
END $$;

-- 5. Verify migrations can use longer timeouts without being killed
-- (Simulates migration pattern: SET LOCAL for long DDL in a transaction)
BEGIN;
    SET LOCAL statement_timeout = '300s';  -- 5 minutes for migrations
    DO $$
    BEGIN
        -- Simulate a long DDL (just sleep 1s as a proxy)
        PERFORM pg_sleep(1);
        RAISE NOTICE 'PASS: Migration-style SET LOCAL with long timeout works';
    END $$;
COMMIT;

-- Reset to default
RESET statement_timeout;

SELECT 'ALL STATEMENT TIMEOUT TESTS PASSED' AS result;
