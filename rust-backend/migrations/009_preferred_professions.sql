-- Add preferred_professions column to user_preferences table
ALTER TABLE user_preferences
ADD COLUMN IF NOT EXISTS preferred_professions JSONB DEFAULT '[]';
