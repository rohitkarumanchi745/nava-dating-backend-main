-- =============================================================================
-- Migration 016: Add Amravati (Maharashtra) colleges + improve searchability
-- =============================================================================

-- Keep VIT AP / SRM AP short_names correct (Amaravati AP ≠ Amravati Maharashtra)
UPDATE universities SET short_name = 'VIT AP' WHERE name = 'VIT-AP University';
UPDATE universities SET short_name = 'SRM AP' WHERE name = 'SRM University AP';

-- Sant Gadge Baba Amravati University (public university, Amravati, Maharashtra)
INSERT INTO universities (name, short_name, domain, country, country_code, state_province, city, tier, university_type)
VALUES ('Sant Gadge Baba Amravati University', 'SGBAU', 'amravati.digitaluniversity.ac', 'India', 'IND', 'Maharashtra', 'Amravati', 'tier2', 'public')
ON CONFLICT (domain) DO NOTHING;

-- Prof Ram Meghe Institute of Technology & Research, Amravati
INSERT INTO universities (name, short_name, domain, country, country_code, state_province, city, tier, university_type)
VALUES ('Prof Ram Meghe Institute of Technology & Research', 'PRMIT&R', 'prmit.in', 'India', 'IND', 'Maharashtra', 'Amravati', 'tier3', 'private')
ON CONFLICT (domain) DO NOTHING;

-- Sipna College of Engineering and Technology, Amravati
INSERT INTO universities (name, short_name, domain, country, country_code, state_province, city, tier, university_type)
VALUES ('Sipna College of Engineering and Technology', 'SCET Amravati', 'sipnaengg.ac.in', 'India', 'IND', 'Maharashtra', 'Amravati', 'tier3', 'private')
ON CONFLICT (domain) DO NOTHING;

-- Government College of Engineering Amravati
INSERT INTO universities (name, short_name, domain, country, country_code, state_province, city, tier, university_type)
VALUES ('Government College of Engineering Amravati', 'GCOEA', 'gcoea.ac.in', 'India', 'IND', 'Maharashtra', 'Amravati', 'tier2', 'public')
ON CONFLICT (domain) DO NOTHING;

-- Amravati University (alias entry for discoverability)
INSERT INTO universities (name, short_name, domain, country, country_code, state_province, city, tier, university_type)
VALUES ('Shri Shivaji College of Engineering and Technology', 'SSCET Amravati', 'sscet.net', 'India', 'IND', 'Maharashtra', 'Amravati', 'tier3', 'private')
ON CONFLICT (domain) DO NOTHING;

-- Jawaharlal Darda Institute of Engineering and Technology
INSERT INTO universities (name, short_name, domain, country, country_code, state_province, city, tier, university_type)
VALUES ('Jawaharlal Darda Institute of Engineering and Technology', 'JDIET Yavatmal', 'jdiet.ac.in', 'India', 'IND', 'Maharashtra', 'Yavatmal', 'tier3', 'private')
ON CONFLICT (domain) DO NOTHING;

-- Tulsiramji Gaikwad-Patil College of Engineering
INSERT INTO universities (name, short_name, domain, country, country_code, state_province, city, tier, university_type)
VALUES ('Tulsiramji Gaikwad-Patil College of Engineering and Technology', 'TGPCET', 'tgpcet.com', 'India', 'IND', 'Maharashtra', 'Nagpur', 'tier3', 'private')
ON CONFLICT (domain) DO NOTHING;

-- Shri Sant Gajanan Maharaj College of Engineering, Shegaon
INSERT INTO universities (name, short_name, domain, country, country_code, state_province, city, tier, university_type)
VALUES ('Shri Sant Gajanan Maharaj College of Engineering', 'SSGMCE', 'ssgmce.ac.in', 'India', 'IND', 'Maharashtra', 'Shegaon', 'tier3', 'private')
ON CONFLICT (domain) DO NOTHING;

-- Vidya Bharati Mahavidyalaya, Amravati
INSERT INTO universities (name, short_name, domain, country, country_code, state_province, city, tier, university_type)
VALUES ('Vidya Bharati Mahavidyalaya Amravati', 'VBM Amravati', 'vbmamravati.ac.in', 'India', 'IND', 'Maharashtra', 'Amravati', 'tier3', 'public')
ON CONFLICT (domain) DO NOTHING;

-- Amravati district engineering colleges under SGBAU
INSERT INTO universities (name, short_name, domain, country, country_code, state_province, city, tier, university_type)
VALUES ('Dr Babasaheb Ambedkar College of Engineering and Research', 'BDCOER', 'bdcoenr.ac.in', 'India', 'IND', 'Maharashtra', 'Nagpur', 'tier3', 'private')
ON CONFLICT (domain) DO NOTHING;

INSERT INTO universities (name, short_name, domain, country, country_code, state_province, city, tier, university_type)
VALUES ('Rajiv Gandhi College of Engineering Research and Technology', 'RGCERT', 'rgcertchanda.ac.in', 'India', 'IND', 'Maharashtra', 'Chandrapur', 'tier3', 'private')
ON CONFLICT (domain) DO NOTHING;

INSERT INTO universities (name, short_name, domain, country, country_code, state_province, city, tier, university_type)
VALUES ('Bapurao Deshmukh College of Engineering', 'BDCE Sewagram', 'bdce.edu.in', 'India', 'IND', 'Maharashtra', 'Wardha', 'tier3', 'private')
ON CONFLICT (domain) DO NOTHING;
