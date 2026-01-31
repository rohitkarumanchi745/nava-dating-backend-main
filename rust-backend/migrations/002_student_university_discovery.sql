-- Migration: Student University Discovery System
-- Adds support for university-based discovery, global pass, and playground features

-- ============================================================================
-- Universities Registry (Global Database)
-- ============================================================================
CREATE TABLE IF NOT EXISTS universities (
    id BIGSERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    short_name VARCHAR(100),
    domain VARCHAR(255) NOT NULL,              -- e.g., "utexas.edu", "harvard.edu"
    country VARCHAR(100) NOT NULL,             -- e.g., "USA", "UK", "India"
    country_code VARCHAR(3) NOT NULL,          -- ISO 3166-1 alpha-3
    state_province VARCHAR(100),               -- e.g., "Texas", "California"
    city VARCHAR(100),
    tier VARCHAR(20) DEFAULT 'regular',        -- 'top_private', 'top_public', 'regular'
    student_count INTEGER,                     -- Approximate enrollment
    is_active BOOLEAN DEFAULT TRUE,
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

-- Indexes for university search
CREATE INDEX IF NOT EXISTS idx_universities_domain ON universities(domain);
CREATE INDEX IF NOT EXISTS idx_universities_country ON universities(country_code, is_active);
CREATE INDEX IF NOT EXISTS idx_universities_name ON universities(name);
CREATE INDEX IF NOT EXISTS idx_universities_tier ON universities(tier);

-- ============================================================================
-- Update student_verifications to link to universities table
-- ============================================================================
ALTER TABLE student_verifications
ADD COLUMN IF NOT EXISTS university_id BIGINT REFERENCES universities(id),
ADD COLUMN IF NOT EXISTS university_country VARCHAR(100),
ADD COLUMN IF NOT EXISTS university_country_code VARCHAR(3),
ADD COLUMN IF NOT EXISTS graduation_year INTEGER,
ADD COLUMN IF NOT EXISTS is_alumni BOOLEAN DEFAULT FALSE;

CREATE INDEX IF NOT EXISTS idx_student_verifications_university ON student_verifications(university_id, status);
CREATE INDEX IF NOT EXISTS idx_student_verifications_country ON student_verifications(university_country_code, status);

-- ============================================================================
-- Student Discovery Passes (University-Based Access)
-- ============================================================================
CREATE TABLE IF NOT EXISTS student_discovery_passes (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    pass_type VARCHAR(30) NOT NULL,            -- 'university', 'country', 'global', 'playground'

    -- Scope of the pass
    university_id BIGINT REFERENCES universities(id),  -- NULL for country/global passes
    country_code VARCHAR(3),                   -- NULL for global passes

    -- Pass details
    status VARCHAR(20) DEFAULT 'active',       -- 'active', 'expired', 'cancelled'
    start_date TIMESTAMP DEFAULT NOW(),
    end_date TIMESTAMP,                        -- NULL for lifetime passes

    -- Pricing
    amount_paid DECIMAL(10, 2),
    currency VARCHAR(3) DEFAULT 'USD',
    payment_id VARCHAR(255),

    -- Usage tracking
    swipes_used INTEGER DEFAULT 0,
    swipes_limit INTEGER,                      -- NULL for unlimited
    last_used_at TIMESTAMP,

    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_student_passes_user ON student_discovery_passes(user_id, status);
CREATE INDEX IF NOT EXISTS idx_student_passes_type ON student_discovery_passes(pass_type, status);
CREATE INDEX IF NOT EXISTS idx_student_passes_university ON student_discovery_passes(university_id, status);
CREATE INDEX IF NOT EXISTS idx_student_passes_country ON student_discovery_passes(country_code, status);

-- ============================================================================
-- Playground Feature (Virtual Social Spaces by University)
-- ============================================================================
CREATE TABLE IF NOT EXISTS playgrounds (
    id BIGSERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    playground_type VARCHAR(30) NOT NULL,      -- 'university', 'city', 'interest', 'event'

    -- Scope
    university_id BIGINT REFERENCES universities(id),
    country_code VARCHAR(3),
    city VARCHAR(100),

    -- Settings
    is_public BOOLEAN DEFAULT TRUE,            -- Public or invite-only
    max_members INTEGER DEFAULT 1000,
    requires_verification BOOLEAN DEFAULT TRUE,

    -- Media
    cover_image_url TEXT,
    icon_url TEXT,

    -- Stats
    member_count INTEGER DEFAULT 0,
    active_today INTEGER DEFAULT 0,

    is_active BOOLEAN DEFAULT TRUE,
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_playgrounds_university ON playgrounds(university_id, is_active);
CREATE INDEX IF NOT EXISTS idx_playgrounds_type ON playgrounds(playground_type, is_active);

-- ============================================================================
-- Playground Memberships
-- ============================================================================
CREATE TABLE IF NOT EXISTS playground_members (
    id BIGSERIAL PRIMARY KEY,
    playground_id BIGINT NOT NULL REFERENCES playgrounds(id) ON DELETE CASCADE,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role VARCHAR(20) DEFAULT 'member',         -- 'member', 'moderator', 'admin'
    joined_at TIMESTAMP DEFAULT NOW(),
    last_active_at TIMESTAMP DEFAULT NOW(),
    is_active BOOLEAN DEFAULT TRUE,
    UNIQUE(playground_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_playground_members_user ON playground_members(user_id, is_active);
CREATE INDEX IF NOT EXISTS idx_playground_members_playground ON playground_members(playground_id, is_active);

-- ============================================================================
-- Student Discovery Swipes (Track cross-university interactions)
-- ============================================================================
CREATE TABLE IF NOT EXISTS student_discovery_swipes (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    target_user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,

    -- Context
    discovery_type VARCHAR(30) NOT NULL,       -- 'university', 'country', 'global', 'playground'
    source_university_id BIGINT REFERENCES universities(id),
    target_university_id BIGINT REFERENCES universities(id),
    playground_id BIGINT REFERENCES playgrounds(id),

    -- Action
    action VARCHAR(20) NOT NULL,               -- 'like', 'pass', 'message'

    -- Pass used
    pass_id BIGINT REFERENCES student_discovery_passes(id),

    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_student_swipes_user ON student_discovery_swipes(user_id, created_at);
CREATE INDEX IF NOT EXISTS idx_student_swipes_discovery ON student_discovery_swipes(discovery_type, created_at);

-- ============================================================================
-- Seed Popular Universities (USA)
-- ============================================================================
INSERT INTO universities (name, short_name, domain, country, country_code, state_province, city, tier) VALUES
-- Ivy League (Top Private)
('Harvard University', 'Harvard', 'harvard.edu', 'USA', 'USA', 'Massachusetts', 'Cambridge', 'top_private'),
('Stanford University', 'Stanford', 'stanford.edu', 'USA', 'USA', 'California', 'Stanford', 'top_private'),
('Massachusetts Institute of Technology', 'MIT', 'mit.edu', 'USA', 'USA', 'Massachusetts', 'Cambridge', 'top_private'),
('Yale University', 'Yale', 'yale.edu', 'USA', 'USA', 'Connecticut', 'New Haven', 'top_private'),
('Princeton University', 'Princeton', 'princeton.edu', 'USA', 'USA', 'New Jersey', 'Princeton', 'top_private'),
('Columbia University', 'Columbia', 'columbia.edu', 'USA', 'USA', 'New York', 'New York', 'top_private'),
('University of Pennsylvania', 'UPenn', 'upenn.edu', 'USA', 'USA', 'Pennsylvania', 'Philadelphia', 'top_private'),
('Brown University', 'Brown', 'brown.edu', 'USA', 'USA', 'Rhode Island', 'Providence', 'top_private'),
('Dartmouth College', 'Dartmouth', 'dartmouth.edu', 'USA', 'USA', 'New Hampshire', 'Hanover', 'top_private'),
('Cornell University', 'Cornell', 'cornell.edu', 'USA', 'USA', 'New York', 'Ithaca', 'top_private'),
('Duke University', 'Duke', 'duke.edu', 'USA', 'USA', 'North Carolina', 'Durham', 'top_private'),
('Northwestern University', 'Northwestern', 'northwestern.edu', 'USA', 'USA', 'Illinois', 'Evanston', 'top_private'),
('University of Chicago', 'UChicago', 'uchicago.edu', 'USA', 'USA', 'Illinois', 'Chicago', 'top_private'),
('Carnegie Mellon University', 'CMU', 'cmu.edu', 'USA', 'USA', 'Pennsylvania', 'Pittsburgh', 'top_private'),

-- Top Public Universities (USA)
('University of California, Berkeley', 'UC Berkeley', 'berkeley.edu', 'USA', 'USA', 'California', 'Berkeley', 'top_public'),
('University of California, Los Angeles', 'UCLA', 'ucla.edu', 'USA', 'USA', 'California', 'Los Angeles', 'top_public'),
('University of Michigan', 'UMich', 'umich.edu', 'USA', 'USA', 'Michigan', 'Ann Arbor', 'top_public'),
('University of Virginia', 'UVA', 'virginia.edu', 'USA', 'USA', 'Virginia', 'Charlottesville', 'top_public'),
('University of North Carolina at Chapel Hill', 'UNC', 'unc.edu', 'USA', 'USA', 'North Carolina', 'Chapel Hill', 'top_public'),
('University of Texas at Austin', 'UT Austin', 'utexas.edu', 'USA', 'USA', 'Texas', 'Austin', 'top_public'),
('University of Texas at Dallas', 'UT Dallas', 'utdallas.edu', 'USA', 'USA', 'Texas', 'Richardson', 'top_public'),
('University of Texas at Arlington', 'UTA', 'uta.edu', 'USA', 'USA', 'Texas', 'Arlington', 'top_public'),
('University of Texas at San Antonio', 'UTSA', 'utsa.edu', 'USA', 'USA', 'Texas', 'San Antonio', 'top_public'),
('Georgia Institute of Technology', 'Georgia Tech', 'gatech.edu', 'USA', 'USA', 'Georgia', 'Atlanta', 'top_public'),
('University of Wisconsin-Madison', 'UW Madison', 'wisc.edu', 'USA', 'USA', 'Wisconsin', 'Madison', 'top_public'),
('University of Illinois Urbana-Champaign', 'UIUC', 'illinois.edu', 'USA', 'USA', 'Illinois', 'Champaign', 'top_public'),
('University of Washington', 'UW', 'washington.edu', 'USA', 'USA', 'Washington', 'Seattle', 'top_public'),
('Purdue University', 'Purdue', 'purdue.edu', 'USA', 'USA', 'Indiana', 'West Lafayette', 'top_public'),
('Ohio State University', 'OSU', 'osu.edu', 'USA', 'USA', 'Ohio', 'Columbus', 'top_public'),
('Penn State University', 'Penn State', 'psu.edu', 'USA', 'USA', 'Pennsylvania', 'University Park', 'top_public'),
('University of Maryland', 'UMD', 'umd.edu', 'USA', 'USA', 'Maryland', 'College Park', 'top_public'),
('University of Minnesota', 'UMN', 'umn.edu', 'USA', 'USA', 'Minnesota', 'Minneapolis', 'top_public'),
('University of Florida', 'UF', 'ufl.edu', 'USA', 'USA', 'Florida', 'Gainesville', 'top_public'),
('Texas A&M University', 'TAMU', 'tamu.edu', 'USA', 'USA', 'Texas', 'College Station', 'top_public'),
('University of Southern California', 'USC', 'usc.edu', 'USA', 'USA', 'California', 'Los Angeles', 'top_public'),
('Arizona State University', 'ASU', 'asu.edu', 'USA', 'USA', 'Arizona', 'Tempe', 'regular'),
('University of Arizona', 'UArizona', 'arizona.edu', 'USA', 'USA', 'Arizona', 'Tucson', 'regular'),
('University of Colorado Boulder', 'CU Boulder', 'colorado.edu', 'USA', 'USA', 'Colorado', 'Boulder', 'regular'),
('Boston University', 'BU', 'bu.edu', 'USA', 'USA', 'Massachusetts', 'Boston', 'regular'),
('New York University', 'NYU', 'nyu.edu', 'USA', 'USA', 'New York', 'New York', 'top_private'),

-- UK Universities
('University of Oxford', 'Oxford', 'ox.ac.uk', 'UK', 'GBR', 'Oxfordshire', 'Oxford', 'top_private'),
('University of Cambridge', 'Cambridge', 'cam.ac.uk', 'UK', 'GBR', 'Cambridgeshire', 'Cambridge', 'top_private'),
('Imperial College London', 'Imperial', 'imperial.ac.uk', 'UK', 'GBR', 'London', 'London', 'top_private'),
('London School of Economics', 'LSE', 'lse.ac.uk', 'UK', 'GBR', 'London', 'London', 'top_private'),
('University College London', 'UCL', 'ucl.ac.uk', 'UK', 'GBR', 'London', 'London', 'top_public'),
('University of Edinburgh', 'Edinburgh', 'ed.ac.uk', 'UK', 'GBR', 'Scotland', 'Edinburgh', 'top_public'),
('University of Manchester', 'Manchester', 'manchester.ac.uk', 'UK', 'GBR', 'Greater Manchester', 'Manchester', 'top_public'),
('Kings College London', 'KCL', 'kcl.ac.uk', 'UK', 'GBR', 'London', 'London', 'top_public'),

-- Canada Universities
('University of Toronto', 'UofT', 'utoronto.ca', 'Canada', 'CAN', 'Ontario', 'Toronto', 'top_public'),
('University of British Columbia', 'UBC', 'ubc.ca', 'Canada', 'CAN', 'British Columbia', 'Vancouver', 'top_public'),
('McGill University', 'McGill', 'mcgill.ca', 'Canada', 'CAN', 'Quebec', 'Montreal', 'top_public'),
('University of Waterloo', 'Waterloo', 'uwaterloo.ca', 'Canada', 'CAN', 'Ontario', 'Waterloo', 'top_public'),
('University of Alberta', 'UAlberta', 'ualberta.ca', 'Canada', 'CAN', 'Alberta', 'Edmonton', 'top_public'),

-- Australia Universities
('University of Melbourne', 'UniMelb', 'unimelb.edu.au', 'Australia', 'AUS', 'Victoria', 'Melbourne', 'top_public'),
('University of Sydney', 'USyd', 'sydney.edu.au', 'Australia', 'AUS', 'New South Wales', 'Sydney', 'top_public'),
('Australian National University', 'ANU', 'anu.edu.au', 'Australia', 'AUS', 'ACT', 'Canberra', 'top_public'),
('University of Queensland', 'UQ', 'uq.edu.au', 'Australia', 'AUS', 'Queensland', 'Brisbane', 'top_public'),
('UNSW Sydney', 'UNSW', 'unsw.edu.au', 'Australia', 'AUS', 'New South Wales', 'Sydney', 'top_public'),
('Monash University', 'Monash', 'monash.edu', 'Australia', 'AUS', 'Victoria', 'Melbourne', 'top_public'),

-- India Universities (IITs, IIMs, Top Universities)
('Indian Institute of Technology Bombay', 'IIT Bombay', 'iitb.ac.in', 'India', 'IND', 'Maharashtra', 'Mumbai', 'top_public'),
('Indian Institute of Technology Delhi', 'IIT Delhi', 'iitd.ac.in', 'India', 'IND', 'Delhi', 'New Delhi', 'top_public'),
('Indian Institute of Technology Madras', 'IIT Madras', 'iitm.ac.in', 'India', 'IND', 'Tamil Nadu', 'Chennai', 'top_public'),
('Indian Institute of Technology Kanpur', 'IIT Kanpur', 'iitk.ac.in', 'India', 'IND', 'Uttar Pradesh', 'Kanpur', 'top_public'),
('Indian Institute of Technology Kharagpur', 'IIT Kharagpur', 'iitkgp.ac.in', 'India', 'IND', 'West Bengal', 'Kharagpur', 'top_public'),
('Indian Institute of Technology Hyderabad', 'IIT Hyderabad', 'iith.ac.in', 'India', 'IND', 'Telangana', 'Hyderabad', 'top_public'),
('Indian Institute of Science', 'IISc', 'iisc.ac.in', 'India', 'IND', 'Karnataka', 'Bangalore', 'top_public'),
('Indian Institute of Management Ahmedabad', 'IIM-A', 'iima.ac.in', 'India', 'IND', 'Gujarat', 'Ahmedabad', 'top_private'),
('Indian Institute of Management Bangalore', 'IIM-B', 'iimb.ac.in', 'India', 'IND', 'Karnataka', 'Bangalore', 'top_private'),
('BITS Pilani', 'BITS', 'bits-pilani.ac.in', 'India', 'IND', 'Rajasthan', 'Pilani', 'top_private'),
('Delhi University', 'DU', 'du.ac.in', 'India', 'IND', 'Delhi', 'New Delhi', 'top_public'),

-- Germany Universities
('Technical University of Munich', 'TUM', 'tum.de', 'Germany', 'DEU', 'Bavaria', 'Munich', 'top_public'),
('Ludwig Maximilian University of Munich', 'LMU', 'lmu.de', 'Germany', 'DEU', 'Bavaria', 'Munich', 'top_public'),
('Heidelberg University', 'Heidelberg', 'uni-heidelberg.de', 'Germany', 'DEU', 'Baden-Wurttemberg', 'Heidelberg', 'top_public'),
('Humboldt University of Berlin', 'HU Berlin', 'hu-berlin.de', 'Germany', 'DEU', 'Berlin', 'Berlin', 'top_public'),
('RWTH Aachen University', 'RWTH', 'rwth-aachen.de', 'Germany', 'DEU', 'North Rhine-Westphalia', 'Aachen', 'top_public'),

-- Singapore Universities
('National University of Singapore', 'NUS', 'nus.edu.sg', 'Singapore', 'SGP', 'Singapore', 'Singapore', 'top_public'),
('Nanyang Technological University', 'NTU', 'ntu.edu.sg', 'Singapore', 'SGP', 'Singapore', 'Singapore', 'top_public')

ON CONFLICT DO NOTHING;

-- ============================================================================
-- Create default playgrounds for universities
-- ============================================================================
INSERT INTO playgrounds (name, description, playground_type, university_id, requires_verification)
SELECT
    u.short_name || ' Singles',
    'Connect with verified students from ' || u.name,
    'university',
    u.id,
    TRUE
FROM universities u
WHERE u.is_active = TRUE
ON CONFLICT DO NOTHING;
