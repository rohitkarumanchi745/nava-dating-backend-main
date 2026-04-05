-- Banner image URLs for user-created social surfaces.
-- iOS PhotoBannerPickerSection uploads base64; backend decodes, persists to
-- /uploads/banners/ and stores the URL path here.

ALTER TABLE events        ADD COLUMN IF NOT EXISTS banner_url TEXT;
ALTER TABLE playgrounds   ADD COLUMN IF NOT EXISTS banner_url TEXT;
ALTER TABLE outdoor_spots ADD COLUMN IF NOT EXISTS banner_url TEXT;
