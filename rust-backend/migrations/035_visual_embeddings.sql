-- 035_visual_embeddings.sql
-- Per-user visual (photo) embeddings from the ImageNet-pretrained backbone in
-- the vision pipeline. Served as a pairwise "visual compatibility" score that
-- fills matches.visual_compatibility_score and blends into the reciprocal
-- matcher (gated by VISUAL_SCORE_WEIGHT). Same portable float8[] storage as the
-- GNN embeddings.

CREATE TABLE IF NOT EXISTS user_visual_embeddings (
    user_id       BIGINT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    embedding     DOUBLE PRECISION[] NOT NULL,
    dim           INTEGER NOT NULL,
    model_version INTEGER NOT NULL DEFAULT 1,
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_visual_emb_version ON user_visual_embeddings(model_version);
