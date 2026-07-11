-- 036_clip_search.sql
-- CoreML-driven photo search. Devices run MobileCLIP (Core ML) and upload their
-- own 512-d image embedding; the server only stores vectors and runs ANN. No
-- server-side vision model.
--
-- Requires the pgvector extension. On Railway, use a pgvector-enabled Postgres
-- (the "pgvector" template) or `CREATE EXTENSION vector` must be permitted.

CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE IF NOT EXISTS user_clip_embeddings (
    user_id       BIGINT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    embedding     vector(512) NOT NULL,
    model_version TEXT NOT NULL DEFAULT 'mobileclip_s2',
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- HNSW index for cosine ANN (pgvector >= 0.5).
CREATE INDEX IF NOT EXISTS idx_clip_emb_hnsw
    ON user_clip_embeddings USING hnsw (embedding vector_cosine_ops);
