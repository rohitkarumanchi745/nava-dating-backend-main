-- =============================================================================
-- Migration 020: Alumni Verification Methods
-- Extends student_verifications to support alumni-specific methods:
--   alumni_email   — OTP to .edu email they still have access to
--   alumni_degree  — Degree/diploma/transcript image upload
--   alumni_linkedin — LinkedIn profile URL (manual review)
-- =============================================================================

-- LinkedIn URL for alumni_linkedin method
ALTER TABLE student_verifications
    ADD COLUMN IF NOT EXISTS linkedin_url TEXT;

-- Degree/diploma document path for alumni_degree method
ALTER TABLE student_verifications
    ADD COLUMN IF NOT EXISTS alumni_doc_path TEXT;

-- Index for fast alumni queue lookups in admin panel
CREATE INDEX IF NOT EXISTS idx_sv_alumni_pending
    ON student_verifications (status, is_alumni, submitted_at DESC)
    WHERE is_alumni = TRUE;
