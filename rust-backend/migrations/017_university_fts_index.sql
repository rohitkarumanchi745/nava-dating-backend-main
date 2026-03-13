-- =============================================================================
-- Migration 017: Full-text search + trigram indexes for university search
-- O(log n) via GIN indexes instead of O(n) sequential scans
-- =============================================================================

CREATE EXTENSION IF NOT EXISTS pg_trgm;

ALTER TABLE universities ADD COLUMN IF NOT EXISTS search_vector tsvector;

-- Populate search_vector: A=name/short_name (highest weight), B=city, C=state
UPDATE universities SET search_vector =
    setweight(to_tsvector('simple', coalesce(name, '')), 'A') ||
    setweight(to_tsvector('english', coalesce(name, '')), 'A') ||
    setweight(to_tsvector('simple', coalesce(short_name, '')), 'A') ||
    setweight(to_tsvector('simple', coalesce(city, '')), 'B') ||
    setweight(to_tsvector('simple', coalesce(state_province, '')), 'C');

-- GIN index for FTS (O(log n))
CREATE INDEX IF NOT EXISTS idx_universities_fts
    ON universities USING GIN(search_vector);

-- Trigram indexes for fuzzy/similarity matching (O(log n))
CREATE INDEX IF NOT EXISTS idx_universities_name_trgm
    ON universities USING GIN(name gin_trgm_ops);

CREATE INDEX IF NOT EXISTS idx_universities_short_trgm
    ON universities USING GIN(short_name gin_trgm_ops);

-- Trigger to auto-update search_vector on insert/update
CREATE OR REPLACE FUNCTION universities_search_vector_update() RETURNS trigger AS $$
BEGIN
    NEW.search_vector :=
        setweight(to_tsvector('simple', coalesce(NEW.name, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(NEW.name, '')), 'A') ||
        setweight(to_tsvector('simple', coalesce(NEW.short_name, '')), 'A') ||
        setweight(to_tsvector('simple', coalesce(NEW.city, '')), 'B') ||
        setweight(to_tsvector('simple', coalesce(NEW.state_province, '')), 'C');
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trig_universities_search_vector ON universities;
CREATE TRIGGER trig_universities_search_vector
    BEFORE INSERT OR UPDATE OF name, short_name, city, state_province
    ON universities
    FOR EACH ROW EXECUTE FUNCTION universities_search_vector_update();
