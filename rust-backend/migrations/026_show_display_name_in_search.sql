-- Opt-in: let users expose their mutable display_name in /search/students
-- results as an alias alongside the verified users.name.
--
-- Default FALSE so search identity stays stable by default.

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS show_display_name_in_search BOOLEAN NOT NULL DEFAULT FALSE;

COMMENT ON COLUMN users.show_display_name_in_search IS
    'When TRUE, /search/students returns display_name as an alias alongside verified users.name.';
