-- Verified identity: split immutable legal name from mutable display name.
--
-- users.name      = verified legal name, captured at student/alumni verification.
--                   Becomes IMMUTABLE after verification. Used for name search.
-- users.display_name = UI-facing name. Mutable. Shown in chat, profile headers.
--                      NOT indexed for search (prevents impersonation).
--
-- Rationale: free-text mutable names make name search unreliable and enable
-- impersonation. With verified users, we can trust users.name as the search key.

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS display_name TEXT;

-- Only full_name gets a search-optimized index. display_name never does.
CREATE INDEX IF NOT EXISTS idx_users_name_lower
    ON users (LOWER(name));

COMMENT ON COLUMN users.name IS
    'Verified legal name. Immutable after student/alumni verification. Search key.';
COMMENT ON COLUMN users.display_name IS
    'Mutable UI name shown in chat/profile. NOT searchable. Falls back to users.name.';
