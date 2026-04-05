-- Per-user toggle: should viewers of this person's public profile see the
-- verified users.name, or only the display_name?
--
-- Default TRUE — verified identity is the Nava trust signal, surface it by
-- default. Users who want to pass only by their display_name can opt out.
--
-- Search is NOT affected by this flag — /search/students still returns
-- verified users.name as the search key, because the whole point of search
-- is finding someone you already know by their real name.

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS show_verified_name BOOLEAN NOT NULL DEFAULT TRUE;

COMMENT ON COLUMN users.show_verified_name IS
    'When TRUE, public profile views expose users.name. When FALSE, viewers see only display_name. Does NOT affect name search.';
