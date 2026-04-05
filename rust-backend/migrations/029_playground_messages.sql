-- Group chat for playgrounds. Members-only (enforced in handlers).

CREATE TABLE IF NOT EXISTS playground_messages (
    id BIGSERIAL PRIMARY KEY,
    playground_id BIGINT NOT NULL REFERENCES playgrounds(id) ON DELETE CASCADE,
    sender_id BIGINT NOT NULL REFERENCES users(id),
    content TEXT NOT NULL CHECK (char_length(content) BETWEEN 1 AND 2000),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Hot path: messages for a playground, newest-first, with 'before' cursor
CREATE INDEX IF NOT EXISTS idx_playground_messages_playground_time
    ON playground_messages (playground_id, created_at DESC);
