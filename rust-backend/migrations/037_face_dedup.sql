-- 037_face_dedup.sql
-- Fake-profile detection: one ArcFace identity embedding per user (from their
-- primary profile photo, computed by the self-hosted ONNX pipeline). ANN
-- search over these finds the same face reused across different accounts —
-- the classic stolen-photo / fake-account pattern. Matches are flagged to
-- trust_safety_events for review, never auto-blocked (twins and shared
-- photos exist).

CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE IF NOT EXISTS user_face_embeddings (
    user_id    BIGINT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    embedding  vector(512) NOT NULL,
    photo_key  TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_face_emb_hnsw
    ON user_face_embeddings USING hnsw (embedding vector_cosine_ops);
