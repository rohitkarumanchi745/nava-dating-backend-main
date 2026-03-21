-- =============================================================================
-- Nava Graph Abstraction — Netflix-inspired property graph store
-- Inspired by: Netflix Tech Blog "Introducing Netflix's Graph Abstraction Layer"
-- Adapted for dating-app scale (millions of users, not 650TB)
--
-- Key design decisions (mirroring Netflix):
--   1. Forward + reverse edge indexes for O(1) traversal in either direction
--   2. Edge links separated from edge properties (prevent wide rows, allow
--      independent property upserts without reading full edge state)
--   3. Direction-agnostic edge_key (sorted node-pair) so properties can be
--      fetched in a single lookup regardless of traversal direction
--   4. graph_delete_queue for async orphan cleanup on node deletion
-- =============================================================================

-- Node types
DO $$ BEGIN
    CREATE TYPE graph_node_type AS ENUM ('user', 'university', 'reel', 'device');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

-- Edge types
DO $$ BEGIN
    CREATE TYPE graph_edge_type AS ENUM (
        'matched_with',   -- User <-> User  (bidirectional)
        'liked',          -- User  -> User
        'passed',         -- User  -> User
        'attends',        -- User  -> University
        'viewed',         -- User  -> Reel
        'created',        -- User  -> Reel
        'uses',           -- User  -> Device
        'messaged'        -- User  -> User
    );
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

-- ---------------------------------------------------------------------------
-- Node property store
-- One row per node; all properties in JSONB (equivalent to Netflix KV record)
-- Efficient reads: single partition lookup returns node + all properties
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS graph_nodes (
    node_type   graph_node_type NOT NULL,
    node_id     TEXT            NOT NULL,
    properties  JSONB           NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ     NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ     NOT NULL DEFAULT NOW(),
    PRIMARY KEY (node_type, node_id)
);

CREATE INDEX IF NOT EXISTS idx_graph_nodes_type
    ON graph_nodes (node_type);

-- ---------------------------------------------------------------------------
-- Forward edge index (adjacency list: source -> neighbors)
-- Enables OUT traversal: "who has user X liked / matched with?"
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS graph_edge_links_fwd (
    from_type   graph_node_type NOT NULL,
    from_id     TEXT            NOT NULL,
    edge_type   graph_edge_type NOT NULL,
    to_type     graph_node_type NOT NULL,
    to_id       TEXT            NOT NULL,
    written_at  TIMESTAMPTZ     NOT NULL DEFAULT NOW(),
    PRIMARY KEY (from_type, from_id, edge_type, to_type, to_id)
);

-- Lookup by (from, edge_type, to_type) — main traversal pattern
CREATE INDEX IF NOT EXISTS idx_gef_fwd_lookup
    ON graph_edge_links_fwd (from_type, from_id, edge_type, to_type);

-- Latest-first sort for "most recent connections" queries
CREATE INDEX IF NOT EXISTS idx_gef_fwd_latest
    ON graph_edge_links_fwd (from_type, from_id, edge_type, written_at DESC);

-- ---------------------------------------------------------------------------
-- Reverse edge index (adjacency list: target -> sources)
-- Enables IN traversal: "who has viewed reel Y? who liked user X?"
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS graph_edge_links_rev (
    to_type     graph_node_type NOT NULL,
    to_id       TEXT            NOT NULL,
    edge_type   graph_edge_type NOT NULL,
    from_type   graph_node_type NOT NULL,
    from_id     TEXT            NOT NULL,
    written_at  TIMESTAMPTZ     NOT NULL DEFAULT NOW(),
    PRIMARY KEY (to_type, to_id, edge_type, from_type, from_id)
);

CREATE INDEX IF NOT EXISTS idx_gef_rev_lookup
    ON graph_edge_links_rev (to_type, to_id, edge_type, from_type);

CREATE INDEX IF NOT EXISTS idx_gef_rev_latest
    ON graph_edge_links_rev (to_type, to_id, edge_type, written_at DESC);

-- ---------------------------------------------------------------------------
-- Edge property index (direction-agnostic)
-- edge_key = LEAST(from_id, to_id) || ':' || GREATEST(from_id, to_id)
-- Separated from link tables to:
--   a) prevent wide-row issues on high-fanout nodes
--   b) allow property upserts without touching link indexes
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS graph_edge_properties (
    edge_key    TEXT            NOT NULL,
    edge_type   graph_edge_type NOT NULL,
    properties  JSONB           NOT NULL DEFAULT '{}',
    written_at  TIMESTAMPTZ     NOT NULL DEFAULT NOW(),
    PRIMARY KEY (edge_key, edge_type)
);

CREATE INDEX IF NOT EXISTS idx_graph_edge_props_lookup
    ON graph_edge_properties (edge_key, edge_type);

-- ---------------------------------------------------------------------------
-- Async node deletion queue
-- Deleting a node in a connected graph requires cleaning up thousands of edges.
-- We queue it asynchronously (Last-Write-Wins semantics prevent stale reads).
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS graph_delete_queue (
    id          BIGSERIAL           PRIMARY KEY,
    node_type   graph_node_type     NOT NULL,
    node_id     TEXT                NOT NULL,
    queued_at   TIMESTAMPTZ         NOT NULL DEFAULT NOW(),
    processed   BOOLEAN             NOT NULL DEFAULT FALSE,
    processed_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_gdq_unprocessed
    ON graph_delete_queue (processed, queued_at)
    WHERE processed = FALSE;

-- ---------------------------------------------------------------------------
-- Seed from existing Postgres data (backfill)
-- Populates graph_edge_links_fwd/rev from existing matches, swipes,
-- student_verifications, reels tables so graph is immediately queryable
-- ---------------------------------------------------------------------------

-- matched_with edges from matches table
INSERT INTO graph_edge_links_fwd (from_type, from_id, edge_type, to_type, to_id, written_at)
SELECT 'user', user1_id::TEXT, 'matched_with', 'user', user2_id::TEXT, created_at
FROM matches WHERE is_active = true
ON CONFLICT DO NOTHING;

INSERT INTO graph_edge_links_rev (to_type, to_id, edge_type, from_type, from_id, written_at)
SELECT 'user', user2_id::TEXT, 'matched_with', 'user', user1_id::TEXT, created_at
FROM matches WHERE is_active = true
ON CONFLICT DO NOTHING;

-- Also reverse direction for bidirectional matched_with
INSERT INTO graph_edge_links_fwd (from_type, from_id, edge_type, to_type, to_id, written_at)
SELECT 'user', user2_id::TEXT, 'matched_with', 'user', user1_id::TEXT, created_at
FROM matches WHERE is_active = true
ON CONFLICT DO NOTHING;

INSERT INTO graph_edge_links_rev (to_type, to_id, edge_type, from_type, from_id, written_at)
SELECT 'user', user1_id::TEXT, 'matched_with', 'user', user2_id::TEXT, created_at
FROM matches WHERE is_active = true
ON CONFLICT DO NOTHING;

-- liked / passed edges from swipes table
INSERT INTO graph_edge_links_fwd (from_type, from_id, edge_type, to_type, to_id, written_at)
SELECT 'user', from_user_id::TEXT,
       CASE WHEN action = 'like' THEN 'liked'::graph_edge_type
            ELSE 'passed'::graph_edge_type END,
       'user', to_user_id::TEXT, created_at
FROM swipes
ON CONFLICT DO NOTHING;

INSERT INTO graph_edge_links_rev (to_type, to_id, edge_type, from_type, from_id, written_at)
SELECT 'user', to_user_id::TEXT,
       CASE WHEN action = 'like' THEN 'liked'::graph_edge_type
            ELSE 'passed'::graph_edge_type END,
       'user', from_user_id::TEXT, created_at
FROM swipes
ON CONFLICT DO NOTHING;

-- attends edges from student_verifications
INSERT INTO graph_edge_links_fwd (from_type, from_id, edge_type, to_type, to_id, written_at)
SELECT 'user', user_id::TEXT, 'attends', 'university', university_id::TEXT, created_at
FROM student_verifications WHERE verified = true
ON CONFLICT DO NOTHING;

INSERT INTO graph_edge_links_rev (to_type, to_id, edge_type, from_type, from_id, written_at)
SELECT 'university', university_id::TEXT, 'attends', 'user', user_id::TEXT, created_at
FROM student_verifications WHERE verified = true
ON CONFLICT DO NOTHING;

-- created edges from reels table
INSERT INTO graph_edge_links_fwd (from_type, from_id, edge_type, to_type, to_id, written_at)
SELECT 'user', user_id::TEXT, 'created', 'reel', id::TEXT, created_at
FROM reels WHERE is_active = true
ON CONFLICT DO NOTHING;

INSERT INTO graph_edge_links_rev (to_type, to_id, edge_type, from_type, from_id, written_at)
SELECT 'reel', id::TEXT, 'created', 'user', user_id::TEXT, created_at
FROM reels WHERE is_active = true
ON CONFLICT DO NOTHING;
