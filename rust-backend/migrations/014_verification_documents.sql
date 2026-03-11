-- =============================================================================
-- Migration 014: Document-based student verification
-- =============================================================================

DO $$ BEGIN
    CREATE TYPE verification_doc_type AS ENUM (
        'student_id_photo',
        'enrollment_letter',
        'fee_receipt',
        'bonafide_certificate',
        'id_selfie'
    );
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
    CREATE TYPE verification_assurance AS ENUM ('high', 'medium', 'low');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
    CREATE TYPE doc_review_status AS ENUM ('pending', 'approved', 'rejected', 'needs_more_info');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE TABLE IF NOT EXISTS verification_documents (
    id                  bigserial PRIMARY KEY,
    user_id             integer NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    verification_id     integer REFERENCES student_verifications(id) ON DELETE SET NULL,
    doc_type            verification_doc_type NOT NULL,
    storage_path        text NOT NULL,
    extracted_name      text,
    extracted_institute text,
    extracted_expiry    date,
    confidence_score    float4 DEFAULT 0,
    face_match_score    float4,
    review_status       doc_review_status NOT NULL DEFAULT 'pending',
    review_notes        text,
    reviewed_by         integer REFERENCES users(id),
    reviewed_at         timestamp,
    created_at          timestamp NOT NULL DEFAULT NOW(),
    expires_at          timestamp
);

CREATE INDEX IF NOT EXISTS idx_vdoc_user    ON verification_documents (user_id, review_status);
CREATE INDEX IF NOT EXISTS idx_vdoc_pending ON verification_documents (review_status, created_at) WHERE review_status = 'pending';
CREATE INDEX IF NOT EXISTS idx_vdoc_expiry  ON verification_documents (expires_at) WHERE expires_at IS NOT NULL;

ALTER TABLE student_verifications
    ADD COLUMN IF NOT EXISTS assurance_level  text DEFAULT 'medium',
    ADD COLUMN IF NOT EXISTS method_detail    text;

CREATE TABLE IF NOT EXISTS campus_ip_checks (
    id            bigserial PRIMARY KEY,
    user_id       integer NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    ip_address    inet NOT NULL,
    university_id bigint REFERENCES universities(id),
    matched       boolean NOT NULL DEFAULT false,
    checked_at    timestamp NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_campus_ip_user ON campus_ip_checks (user_id, matched);
