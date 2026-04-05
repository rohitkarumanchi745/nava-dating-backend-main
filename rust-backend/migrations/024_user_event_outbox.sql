-- User event outbox for durable /ws/events delivery.
-- Writes happen before broadcast, so clients can replay any missed events
-- on reconnect using ?since=<last_event_id>.
--
-- Retention: 7 days. Background prune job in main.rs runs hourly.

CREATE TABLE IF NOT EXISTS user_event_outbox (
    id BIGSERIAL PRIMARY KEY,
    user_id INTEGER NOT NULL,
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT NOW()
);

-- Hot path: WHERE user_id = $1 AND id > $2 ORDER BY id ASC
CREATE INDEX IF NOT EXISTS idx_user_event_outbox_user_id
    ON user_event_outbox (user_id, id);

-- Prune path: WHERE created_at < NOW() - INTERVAL '7 days'
CREATE INDEX IF NOT EXISTS idx_user_event_outbox_created_at
    ON user_event_outbox (created_at);
