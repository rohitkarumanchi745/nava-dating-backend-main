-- HLS adaptive streaming columns for spots. Mirrors 022_hls_streaming.sql,
-- which added the same columns to reels. Spots now share the reels HLS
-- pipeline so cellular viewers get adaptive bitrate instead of the raw upload.

ALTER TABLE spots
    ADD COLUMN IF NOT EXISTS hls_url   TEXT,
    ADD COLUMN IF NOT EXISTS hls_state VARCHAR(20) NOT NULL DEFAULT 'pending';

-- Existing spots have no HLS yet — mark failed so iOS falls back to original_url.
UPDATE spots SET hls_state = 'failed' WHERE hls_url IS NULL;

-- Index for the transcoding worker / observability to find pending/processing jobs.
CREATE INDEX IF NOT EXISTS idx_spots_hls_pending
    ON spots(hls_state)
    WHERE hls_state IN ('pending', 'processing');
