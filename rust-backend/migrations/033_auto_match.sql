-- 033_auto_match.sql
-- Agentic auto-matching: the matchmaker proposes (or instantly creates) matches
-- from reciprocal preference scores learned from swipes/interactions — no manual
-- swiping required. Proposals live here until the user accepts/declines; their
-- response is fed back to the RL model as a reward signal.

CREATE TABLE IF NOT EXISTS auto_match_suggestions (
    id            SERIAL PRIMARY KEY,
    user_id       BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    candidate_id  BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    mutual_score  DOUBLE PRECISION NOT NULL,     -- reciprocal (both-sided) score
    forward_score DOUBLE PRECISION,              -- P(user likes candidate)
    reverse_score DOUBLE PRECISION,              -- P(candidate likes user)
    -- pending | accepted | declined | expired | auto_matched
    status        TEXT NOT NULL DEFAULT 'pending',
    match_id      VARCHAR(36),                   -- set once a real match exists
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    responded_at  TIMESTAMPTZ
);

-- One open proposal per (user, candidate) pair.
CREATE UNIQUE INDEX IF NOT EXISTS idx_auto_match_pending
    ON auto_match_suggestions(user_id, candidate_id) WHERE status = 'pending';

CREATE INDEX IF NOT EXISTS idx_auto_match_user_status
    ON auto_match_suggestions(user_id, status, created_at DESC);
