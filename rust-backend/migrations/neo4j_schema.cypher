// =============================================================================
// NAVA Platform - Neo4j Graph Database Schema
// =============================================================================
// This schema defines the graph structure for:
// - Social Graph (likes, matches, blocks)
// - University Network Relationships
// - Interest-based Recommendations
// - Fraud Detection Patterns
// - Friend-of-Friend Discovery
// =============================================================================

// -----------------------------------------------------------------------------
// CONSTRAINTS - Ensure data integrity
// -----------------------------------------------------------------------------

// User node constraints
CREATE CONSTRAINT user_id_unique IF NOT EXISTS FOR (u:User) REQUIRE u.id IS UNIQUE;
CREATE CONSTRAINT user_phone_unique IF NOT EXISTS FOR (u:User) REQUIRE u.phone IS UNIQUE;

// University node constraints
CREATE CONSTRAINT university_id_unique IF NOT EXISTS FOR (uni:University) REQUIRE uni.id IS UNIQUE;
CREATE CONSTRAINT university_domain_unique IF NOT EXISTS FOR (uni:University) REQUIRE uni.email_domain IS UNIQUE;

// Country node constraints
CREATE CONSTRAINT country_code_unique IF NOT EXISTS FOR (c:Country) REQUIRE c.code IS UNIQUE;

// Interest node constraints
CREATE CONSTRAINT interest_id_unique IF NOT EXISTS FOR (i:Interest) REQUIRE i.id IS UNIQUE;
CREATE CONSTRAINT interest_name_unique IF NOT EXISTS FOR (i:Interest) REQUIRE i.name IS UNIQUE;

// City node constraints
CREATE CONSTRAINT city_id_unique IF NOT EXISTS FOR (c:City) REQUIRE c.id IS UNIQUE;

// Spot (Reel) node constraints
CREATE CONSTRAINT spot_id_unique IF NOT EXISTS FOR (s:Spot) REQUIRE s.id IS UNIQUE;

// Conversation node constraints
CREATE CONSTRAINT conversation_id_unique IF NOT EXISTS FOR (c:Conversation) REQUIRE c.id IS UNIQUE;

// -----------------------------------------------------------------------------
// INDEXES - Optimize query performance
// -----------------------------------------------------------------------------

// User indexes for common queries
CREATE INDEX user_gender IF NOT EXISTS FOR (u:User) ON (u.gender);
CREATE INDEX user_age IF NOT EXISTS FOR (u:User) ON (u.date_of_birth);
CREATE INDEX user_verified IF NOT EXISTS FOR (u:User) ON (u.is_verified);
CREATE INDEX user_premium IF NOT EXISTS FOR (u:User) ON (u.is_premium);
CREATE INDEX user_student IF NOT EXISTS FOR (u:User) ON (u.is_student);
CREATE INDEX user_active IF NOT EXISTS FOR (u:User) ON (u.is_active);
CREATE INDEX user_last_active IF NOT EXISTS FOR (u:User) ON (u.last_active_at);
CREATE INDEX user_location IF NOT EXISTS FOR (u:User) ON (u.latitude, u.longitude);

// University indexes
CREATE INDEX university_name IF NOT EXISTS FOR (uni:University) ON (uni.name);
CREATE INDEX university_tier IF NOT EXISTS FOR (uni:University) ON (uni.tier);
CREATE INDEX university_type IF NOT EXISTS FOR (uni:University) ON (uni.university_type);

// Interest indexes
CREATE INDEX interest_category IF NOT EXISTS FOR (i:Interest) ON (i.category);

// Spot indexes
CREATE INDEX spot_created IF NOT EXISTS FOR (s:Spot) ON (s.created_at);
CREATE INDEX spot_global IF NOT EXISTS FOR (s:Spot) ON (s.is_global);

// -----------------------------------------------------------------------------
// NODE LABELS AND PROPERTIES
// -----------------------------------------------------------------------------

// User Node Properties:
// - id: UUID (from PostgreSQL users.id)
// - phone: String (unique phone number)
// - name: String
// - gender: String (male, female, non_binary)
// - date_of_birth: Date
// - bio: String
// - latitude: Float
// - longitude: Float
// - city: String
// - is_verified: Boolean
// - is_premium: Boolean
// - is_student: Boolean
// - is_active: Boolean
// - created_at: DateTime
// - last_active_at: DateTime
// - profile_completion: Float (0.0 - 1.0)

// University Node Properties:
// - id: Integer (from PostgreSQL universities.id)
// - name: String
// - email_domain: String
// - country_code: String
// - tier: String (top_private, top_public, regular)
// - university_type: String (private, public)
// - student_count: Integer (approximate)

// Country Node Properties:
// - code: String (ISO 3166-1 alpha-2)
// - name: String
// - continent: String

// Interest Node Properties:
// - id: Integer
// - name: String
// - category: String (music, sports, movies, hobbies, food, travel, etc.)
// - popularity_score: Float

// City Node Properties:
// - id: String (slug: country_code-city_name)
// - name: String
// - country_code: String
// - latitude: Float
// - longitude: Float

// Spot Node Properties:
// - id: UUID
// - user_id: UUID
// - video_url: String
// - thumbnail_url: String
// - caption: String
// - is_global: Boolean
// - city: String
// - like_count: Integer
// - view_count: Integer
// - created_at: DateTime

// -----------------------------------------------------------------------------
// RELATIONSHIP TYPES
// -----------------------------------------------------------------------------

// Social Graph Relationships
// --------------------------

// LIKED: User liked another user's profile
// Properties: created_at, source (swipe, reel, discover)
// (user1)-[:LIKED {created_at: datetime(), source: 'swipe'}]->(user2)

// PASSED: User passed on another user's profile
// Properties: created_at
// (user1)-[:PASSED {created_at: datetime()}]->(user2)

// MATCHED: Mutual like creates a match (bidirectional for queries)
// Properties: created_at, is_active
// (user1)-[:MATCHED {created_at: datetime(), is_active: true}]->(user2)

// BLOCKED: User blocked another user
// Properties: created_at, reason
// (user1)-[:BLOCKED {created_at: datetime(), reason: 'harassment'}]->(user2)

// REPORTED: User reported another user
// Properties: created_at, reason, status
// (user1)-[:REPORTED {created_at: datetime(), reason: 'fake_profile', status: 'pending'}]->(user2)

// MESSAGED: Users have exchanged messages
// Properties: message_count, last_message_at
// (user1)-[:MESSAGED {message_count: 10, last_message_at: datetime()}]->(user2)

// University Network Relationships
// --------------------------------

// STUDIES_AT: User is a verified student at university
// Properties: verified_at, graduation_year, degree
// (user)-[:STUDIES_AT {verified_at: datetime(), graduation_year: 2025, degree: 'BS'}]->(university)

// ALUMNI_OF: User graduated from university
// Properties: graduation_year, degree
// (user)-[:ALUMNI_OF {graduation_year: 2020, degree: 'MS'}]->(university)

// LOCATED_IN: University is in a country
// (university)-[:LOCATED_IN]->(country)

// Interest Relationships
// ----------------------

// INTERESTED_IN: User has an interest
// Properties: priority (1-5), added_at
// (user)-[:INTERESTED_IN {priority: 1, added_at: datetime()}]->(interest)

// SIMILAR_TO: Interest is similar to another interest
// Properties: similarity_score
// (interest1)-[:SIMILAR_TO {similarity_score: 0.85}]->(interest2)

// Location Relationships
// ----------------------

// LIVES_IN: User's current city
// (user)-[:LIVES_IN]->(city)

// CITY_IN: City is in a country
// (city)-[:CITY_IN]->(country)

// Content Relationships
// ---------------------

// CREATED: User created a spot/reel
// (user)-[:CREATED]->(spot)

// LIKED_SPOT: User liked a spot
// Properties: created_at
// (user)-[:LIKED_SPOT {created_at: datetime()}]->(spot)

// VIEWED: User viewed a spot
// Properties: view_duration_seconds, viewed_at
// (user)-[:VIEWED {view_duration_seconds: 15, viewed_at: datetime()}]->(spot)

// -----------------------------------------------------------------------------
// SAMPLE DATA CREATION QUERIES (For Testing)
// -----------------------------------------------------------------------------

// Create sample countries
// MERGE (usa:Country {code: 'US', name: 'United States', continent: 'North America'})
// MERGE (india:Country {code: 'IN', name: 'India', continent: 'Asia'})
// MERGE (uk:Country {code: 'GB', name: 'United Kingdom', continent: 'Europe'})

// Create sample interest categories
// MERGE (music:Interest {id: 1, name: 'Music', category: 'entertainment'})
// MERGE (hiking:Interest {id: 2, name: 'Hiking', category: 'outdoor'})
// MERGE (photography:Interest {id: 3, name: 'Photography', category: 'creative'})

// -----------------------------------------------------------------------------
// GRAPH ALGORITHMS - Useful Queries
// -----------------------------------------------------------------------------

// Friend of Friend Discovery (2-hop matches)
// MATCH (me:User {id: $user_id})-[:MATCHED]-(friend)-[:MATCHED]-(fof:User)
// WHERE NOT (me)-[:MATCHED|BLOCKED|PASSED]-(fof) AND me <> fof
// RETURN fof, count(friend) as mutual_connections
// ORDER BY mutual_connections DESC
// LIMIT 20

// University Network Discovery
// MATCH (me:User {id: $user_id})-[:STUDIES_AT]->(uni:University)<-[:STUDIES_AT]-(classmate:User)
// WHERE NOT (me)-[:MATCHED|BLOCKED|PASSED]-(classmate) AND me <> classmate
// RETURN classmate
// LIMIT 50

// Interest-based Recommendations
// MATCH (me:User {id: $user_id})-[:INTERESTED_IN]->(i:Interest)<-[:INTERESTED_IN]-(similar:User)
// WHERE NOT (me)-[:MATCHED|BLOCKED|PASSED]-(similar) AND me <> similar
// WITH similar, count(i) as shared_interests
// ORDER BY shared_interests DESC
// RETURN similar, shared_interests
// LIMIT 30

// Fraud Detection - Accounts from same device/IP with mutual blocks
// MATCH (u1:User)-[:BLOCKED]->(u2:User)-[:BLOCKED]->(u3:User)
// WHERE u1.device_id = u3.device_id
// RETURN u1, u2, u3

// Popular Users in Network (PageRank-style)
// MATCH (u:User)<-[:LIKED]-(other)
// WITH u, count(other) as incoming_likes
// RETURN u.id, u.name, incoming_likes
// ORDER BY incoming_likes DESC
// LIMIT 100

// Mutual Interest Strength
// MATCH (u1:User {id: $user1_id})-[:INTERESTED_IN]->(i:Interest)<-[:INTERESTED_IN]-(u2:User {id: $user2_id})
// RETURN count(i) as shared_interests, collect(i.name) as interests

// -----------------------------------------------------------------------------
// MAINTENANCE QUERIES
// -----------------------------------------------------------------------------

// Clear all data (USE WITH CAUTION)
// MATCH (n) DETACH DELETE n

// Count all nodes by label
// CALL db.labels() YIELD label
// CALL apoc.cypher.run('MATCH (n:' + label + ') RETURN count(n) as count', {}) YIELD value
// RETURN label, value.count as count

// Count all relationships by type
// CALL db.relationshipTypes() YIELD relationshipType
// CALL apoc.cypher.run('MATCH ()-[r:' + relationshipType + ']->() RETURN count(r) as count', {}) YIELD value
// RETURN relationshipType, value.count as count
