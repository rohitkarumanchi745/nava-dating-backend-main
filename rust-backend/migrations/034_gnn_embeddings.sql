-- 034_gnn_embeddings.sql
-- Graph neural network (GNN) user embeddings.
--
-- Offline-trained embeddings (scripts/gnn_trainer.py) that capture higher-order
-- interaction-graph structure — multi-hop and community signal beyond the
-- first-order co-like CF we already compute. Served cheaply as a pairwise score
-- feeding the reciprocal matcher. Stored as a plain float8[] so no pgvector
-- extension is required (we only do pairwise lookups, not ANN). Swap to pgvector
-- + an ivfflat index if/when you need approximate nearest-neighbour retrieval.

CREATE TABLE IF NOT EXISTS user_graph_embeddings (
    user_id       BIGINT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    embedding     DOUBLE PRECISION[] NOT NULL,
    dim           INTEGER NOT NULL,
    model_version INTEGER NOT NULL DEFAULT 1,
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_gnn_emb_version ON user_graph_embeddings(model_version);
