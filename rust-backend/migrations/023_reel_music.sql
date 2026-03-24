-- Apple Music integration for reels
ALTER TABLE reels
    ADD COLUMN IF NOT EXISTS music_id         VARCHAR(100),  -- Apple Music song ID
    ADD COLUMN IF NOT EXISTS music_title      VARCHAR(255),  -- Song name
    ADD COLUMN IF NOT EXISTS music_artist     VARCHAR(255),  -- Artist name
    ADD COLUMN IF NOT EXISTS music_artwork_url TEXT,          -- Album art URL
    ADD COLUMN IF NOT EXISTS music_preview_url TEXT,          -- 30s preview URL from Apple Music
    ADD COLUMN IF NOT EXISTS music_duration_ms INTEGER,       -- Full song duration
    ADD COLUMN IF NOT EXISTS music_start_ms   INTEGER DEFAULT 0, -- Where in the song the reel starts
    ADD COLUMN IF NOT EXISTS music_genre      VARCHAR(100);  -- Genre for recommendations

CREATE INDEX IF NOT EXISTS idx_reels_music_id ON reels(music_id) WHERE music_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_reels_music_artist ON reels(music_artist) WHERE music_artist IS NOT NULL;
