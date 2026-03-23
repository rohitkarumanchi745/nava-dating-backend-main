-- HLS adaptive streaming columns for reels
ALTER TABLE reels
    ADD COLUMN IF NOT EXISTS hls_url   TEXT,
    ADD COLUMN IF NOT EXISTS hls_state VARCHAR(20) NOT NULL DEFAULT 'pending';

-- Existing reels have no HLS — mark failed so iOS falls back to video_url
UPDATE reels SET hls_state = 'failed' WHERE hls_url IS NULL;

-- Index for the transcoding worker to find pending/processing jobs quickly
CREATE INDEX IF NOT EXISTS idx_reels_hls_pending
    ON reels(hls_state)
    WHERE hls_state IN ('pending', 'processing');
