-- Migration 015: Widen profile photo columns from varchar(500) to text
-- varchar(500) caused "value too long" errors when storing photo paths/URLs
ALTER TABLE users
    ALTER COLUMN profile_photo_1 TYPE text,
    ALTER COLUMN profile_photo_2 TYPE text,
    ALTER COLUMN profile_photo_3 TYPE text;
