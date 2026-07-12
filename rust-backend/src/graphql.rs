use async_graphql::{
    Context, EmptySubscription, Error, InputObject, Object, Result, Schema, SimpleObject, Upload,
    dataloader::{DataLoader, Loader},
};
use chrono::NaiveDateTime;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;

use crate::auth::create_access_token;
use crate::handlers::auto_queue_for_labeling;
use crate::state::AppState;

// ============================================================================
// GraphQL Types
// ============================================================================

#[derive(SimpleObject, Clone, Debug)]
pub struct User {
    pub id: i64,
    pub phone_number: Option<String>,
    pub email: Option<String>,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub show_verified_name: bool,
    pub show_display_name_in_search: bool,
    pub age: Option<i32>,
    pub gender: Option<String>,
    pub bio: Option<String>,
    pub location: Option<String>,
    pub interests: Vec<String>,
    pub languages: Vec<String>,
    pub looking_for: Option<String>,
    pub profession_category: Option<String>,
    pub profession_title: Option<String>,
    pub height_cm: Option<i32>,
    pub photos: Vec<String>,
    pub is_profile_complete: bool,
    pub is_verified: bool,
    pub is_student_verified: bool,
    pub attractiveness_score: Option<f64>,
}

#[derive(SimpleObject, Clone, Debug)]
pub struct UserPreferences {
    pub min_age: Option<i32>,
    pub max_age: Option<i32>,
    pub preferred_genders: Vec<String>,
    pub preferred_professions: Vec<String>,
    pub max_distance_km: Option<i32>,
    pub only_verified: bool,
    pub only_students: bool,
}

#[derive(SimpleObject, Clone, Debug)]
pub struct Match {
    pub id: String,
    pub user1_id: i64,
    pub user2_id: i64,
    pub is_mutual: bool,
    pub status: Option<String>,
    pub matched_at: Option<String>,
    pub partner: Option<User>,
    pub like_type: Option<String>,  // "swipe" | "super_like" | "message_request"
    pub message: Option<String>,    // intro message if sent with like
}

#[derive(SimpleObject, Clone, Debug)]
pub struct LikedProfile {
    pub id: i64,
    pub name: Option<String>,
    pub age: Option<i32>,
    pub photo: Option<String>,
    pub university: Option<String>,
    pub study: Option<String>,
    pub like_type: String,          // "like" | "super_like"
    pub message: Option<String>,    // intro message if sent with like
    pub liked_at: Option<String>,
}

#[derive(SimpleObject, Clone, Debug)]
pub struct DiscoverProfile {
    pub id: i64,
    pub name: Option<String>,
    pub age: Option<i32>,
    pub gender: Option<String>,
    pub bio: Option<String>,
    pub location: Option<String>,
    pub photos: Vec<String>,
    pub interests: Vec<String>,
    pub compatibility_score: Option<f64>,
    pub distance_km: Option<f64>,
    pub is_verified: bool,
    // Voice intro fields
    pub voice_intro_url: Option<String>,
    pub voice_intro_duration: Option<i32>,
    pub has_voice_intro: bool,
    // Enhanced profile fields
    pub profession_title: Option<String>,
    pub languages: Vec<String>,
    pub looking_for: Option<String>,
    pub height_cm: Option<i32>,
}

#[derive(SimpleObject, Clone, Debug)]
pub struct LikeResult {
    pub success: bool,
    pub is_mutual: bool,
    pub match_id: Option<String>,
    pub message: String,
}

#[derive(SimpleObject, Clone, Debug)]
pub struct VoiceIntroResult {
    pub success: bool,
    pub url: String,
    pub duration: i32,
}

#[derive(SimpleObject, Clone, Debug)]
pub struct Message {
    pub id: i64,
    pub match_id: String,
    pub sender_id: i64,
    pub receiver_id: i64,
    pub content: String,
    pub created_at: Option<String>,
    pub is_read: bool,
}

#[derive(SimpleObject, Clone, Debug)]
pub struct AuthPayload {
    pub access_token: String,
    pub user_id: i64,
    pub is_new_user: bool,
    pub is_profile_complete: bool,
}

#[derive(SimpleObject, Clone, Debug)]
pub struct OtpResponse {
    pub message: String,
    pub otp: Option<String>, // Only in development
}

#[derive(SimpleObject, Clone, Debug)]
pub struct StudentStatus {
    pub is_verified: bool,
    pub university_name: Option<String>,
    pub discount_tier: Option<String>,
    pub discount_percent: i32,
    pub expires_at: Option<String>,
}

#[derive(SimpleObject, Clone, Debug)]
pub struct ProfessionOption {
    pub id: String,
    pub category: String,
    pub title: String,
}

#[derive(SimpleObject, Clone, Debug)]
pub struct UniversityResult {
    pub id: i64,
    pub name: String,
    pub domain: Option<String>,
    pub country: Option<String>,
    pub tier: Option<String>,
    pub student_count: i64,
}

#[derive(SimpleObject, Clone, Debug)]
pub struct UniversityProfileResult {
    pub university: UniversityInfo,
    pub profiles: Vec<DiscoverProfile>,
    pub total: i64,
}

#[derive(SimpleObject, Clone, Debug)]
pub struct UniversityInfo {
    pub id: i64,
    pub name: String,
    pub domain: Option<String>,
    pub country: Option<String>,
    pub tier: Option<String>,
}

#[derive(SimpleObject, Clone, Debug)]
pub struct UnifiedSearchResult {
    pub universities: Vec<UniversityResult>,
    pub profiles: Vec<DiscoverProfile>,
}

// ============================================================================
// Input Types
// ============================================================================

#[derive(InputObject)]
pub struct UpdateProfileInput {
    pub name: Option<String>,
    pub gender: Option<String>,
    pub dob: Option<String>,
    pub bio: Option<String>,
    pub interests: Option<Vec<String>>,
    pub languages: Option<Vec<String>>,
    pub looking_for: Option<String>,
    pub profession_category: Option<String>,
    pub profession_title: Option<String>,
    pub height_cm: Option<i32>,
    pub photos: Option<Vec<String>>,
    pub university: Option<String>,
    pub university_location: Option<String>,
    pub study: Option<String>,
}

#[derive(InputObject)]
pub struct PreferencesInput {
    pub min_age: Option<i32>,
    pub max_age: Option<i32>,
    pub preferred_genders: Option<Vec<String>>,
    pub preferred_professions: Option<Vec<String>>,
    pub max_distance_km: Option<i32>,
    pub only_verified: Option<bool>,
    pub only_students: Option<bool>,
}

#[derive(InputObject)]
pub struct DiscoverFilters {
    pub min_age: Option<i32>,
    pub max_age: Option<i32>,
    pub max_distance_km: Option<i32>,
    pub preferred_genders: Option<Vec<String>>,
    pub only_verified: Option<bool>,
    pub use_ai: Option<bool>,
    pub limit: Option<i32>,
}

// ============================================================================
// DataLoader for batching user queries
// ============================================================================

pub struct UserLoader {
    pub pool: PgPool,
}

impl Loader<i64> for UserLoader {
    type Value = User;
    type Error = Arc<sqlx::Error>;

    async fn load(&self, keys: &[i64]) -> Result<HashMap<i64, Self::Value>, Self::Error> {
        let int_keys: Vec<i64> = keys.to_vec();
        let rows = sqlx::query_as::<_, UserRow>(
            r#"
            SELECT id, phone_number, email, name, display_name, show_verified_name, show_display_name_in_search,
                   dob, gender, bio, location_text,
                   interests, languages, looking_for, profession_category, profession_title,
                   height_cm, profile_photo_1, profile_photo_2, profile_photo_3, profile_photos,
                   is_profile_complete, is_verified, is_student_verified, attractiveness_score
            FROM users WHERE id = ANY($1)
            "#,
        )
        .bind(&int_keys)
        .fetch_all(&self.pool)
        .await
        .map_err(Arc::new)?;

        Ok(rows.into_iter().map(|r| (r.id, r.into())).collect())
    }
}

#[derive(sqlx::FromRow)]
struct UserRow {
    id: i64,
    phone_number: Option<String>,
    email: Option<String>,
    name: Option<String>,
    display_name: Option<String>,
    show_verified_name: Option<bool>,
    show_display_name_in_search: Option<bool>,
    dob: Option<chrono::NaiveDate>,
    gender: Option<String>,
    bio: Option<String>,
    location_text: Option<String>,
    interests: Option<serde_json::Value>,
    languages: Option<serde_json::Value>,
    looking_for: Option<String>,
    profession_category: Option<String>,
    profession_title: Option<String>,
    height_cm: Option<i32>,
    profile_photo_1: Option<String>,
    profile_photo_2: Option<String>,
    profile_photo_3: Option<String>,
    profile_photos: Option<serde_json::Value>,
    is_profile_complete: Option<bool>,
    is_verified: Option<bool>,
    is_student_verified: Option<bool>,
    attractiveness_score: Option<rust_decimal::Decimal>,
}

impl From<UserRow> for User {
    fn from(r: UserRow) -> Self {
        // Try to get photos from JSONB first, then fall back to individual columns
        let photos: Vec<String> = r.profile_photos
            .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
            .unwrap_or_else(|| {
                [r.profile_photo_1, r.profile_photo_2, r.profile_photo_3]
                    .into_iter()
                    .flatten()
                    .collect()
            });

        let interests = r.interests
            .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
            .unwrap_or_default();

        let languages = r.languages
            .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
            .unwrap_or_default();

        let age = r.dob.map(|dob| {
            let today = chrono::Utc::now().date_naive();
            let mut age = today.year() - dob.year();
            if today.ordinal() < dob.ordinal() {
                age -= 1;
            }
            age
        });

        User {
            id: r.id,
            phone_number: r.phone_number,
            email: r.email,
            name: r.name,
            display_name: r.display_name,
            show_verified_name: r.show_verified_name.unwrap_or(true),
            show_display_name_in_search: r.show_display_name_in_search.unwrap_or(false),
            age,
            gender: r.gender,
            bio: r.bio,
            location: r.location_text,
            interests,
            languages,
            looking_for: r.looking_for,
            profession_category: r.profession_category,
            profession_title: r.profession_title,
            height_cm: r.height_cm,
            photos,
            is_profile_complete: r.is_profile_complete.unwrap_or(false),
            is_verified: r.is_verified.unwrap_or(false),
            is_student_verified: r.is_student_verified.unwrap_or(false),
            attractiveness_score: r.attractiveness_score.map(|d| d.to_string().parse().unwrap_or(0.0)),
        }
    }
}

use chrono::Datelike;

// ============================================================================
// Query Root
// ============================================================================

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    /// Get current authenticated user's profile
    async fn me(&self, ctx: &Context<'_>) -> Result<Option<User>> {
        let _state = ctx.data::<AppState>()?;
        let user_id = get_user_id_from_context(ctx)?;

        let loader = ctx.data::<DataLoader<UserLoader>>()?;
        let user = loader.load_one(user_id).await?;
        Ok(user)
    }

    /// Get user by ID
    async fn user(&self, ctx: &Context<'_>, id: i64) -> Result<Option<User>> {
        let loader = ctx.data::<DataLoader<UserLoader>>()?;
        let user = loader.load_one(id).await?;
        Ok(user)
    }

    /// Get current user's preferences
    async fn my_preferences(&self, ctx: &Context<'_>) -> Result<Option<UserPreferences>> {
        let state = ctx.data::<AppState>()?;
        let user_id = get_user_id_from_context(ctx)?;

        let row = sqlx::query_as::<_, PreferencesRow>(
            r#"
            SELECT min_age, max_age, preferred_genders, preferred_professions, max_distance, only_verified, only_students
            FROM user_preferences WHERE user_id = $1
            "#,
        )
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?;

        Ok(row.map(|r| UserPreferences {
            min_age: r.min_age,
            max_age: r.max_age,
            preferred_genders: r.preferred_genders
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
            preferred_professions: r.preferred_professions
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
            max_distance_km: r.max_distance,
            only_verified: r.only_verified.unwrap_or(false),
            only_students: r.only_students.unwrap_or(false),
        }))
    }

    /// Discover profiles for matching with AI-powered compatibility scoring
    async fn discover(&self, ctx: &Context<'_>, filters: Option<DiscoverFilters>) -> Result<Vec<DiscoverProfile>> {
        let state = ctx.data::<AppState>()?;
        let user_id = get_user_id_from_context(ctx)?;

        let f = filters.unwrap_or(DiscoverFilters {
            min_age: None,
            max_age: None,
            max_distance_km: None,
            preferred_genders: None,
            only_verified: None,
            use_ai: Some(true),
            limit: Some(20),
        });

        let limit = f.limit.unwrap_or(20).min(50);

        // Run all 3 DB queries in parallel
        let (current_user, rows, preferred_professions) = tokio::try_join!(
            // Query 1: Current user's profile for compatibility
            sqlx::query_as::<_, UserCompatibilityRow>(
                "SELECT interests, languages, looking_for, gender FROM users WHERE id = $1"
            )
            .bind(user_id)
            .fetch_optional(&state.db),

            // Query 2: Discover profiles — uses LEFT JOIN instead of NOT IN for speed
            sqlx::query_as::<_, DiscoverRow>(
                r#"
                SELECT u.id, u.name, u.dob, u.gender, u.bio, u.location_text,
                       u.profile_photo_1, u.profile_photo_2, u.profile_photo_3, u.profile_photos,
                       u.interests, u.languages, u.is_verified, u.attractiveness_score,
                       u.voice_intro_url, u.voice_intro_duration,
                       u.profession_title, u.profession_category, u.looking_for, u.height_cm
                FROM users u
                LEFT JOIN swipes s ON s.from_user_id = $1 AND s.to_user_id = u.id
                WHERE u.id != $1
                  AND u.is_profile_complete = true
                  AND u.is_active = true
                  AND s.from_user_id IS NULL
                ORDER BY
                    CASE WHEN u.voice_intro_url IS NOT NULL THEN 0 ELSE 1 END,
                    u.attractiveness_score DESC NULLS LAST,
                    u.last_active DESC NULLS LAST,
                    u.created_at DESC
                LIMIT $2
                "#,
            )
            .bind(user_id)
            .bind(limit)
            .fetch_all(&state.db),

            // Query 3: User preferences
            sqlx::query_scalar::<_, Option<serde_json::Value>>(
                "SELECT preferred_professions FROM user_preferences WHERE user_id = $1"
            )
            .bind(user_id)
            .fetch_optional(&state.db),
        )?;

        // Calculate compatibility scores
        let current_interests: Vec<String> = current_user.as_ref()
            .and_then(|u| u.interests.as_ref())
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let current_languages: Vec<String> = current_user.as_ref()
            .and_then(|u| u.languages.as_ref())
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let current_looking_for = current_user.as_ref()
            .and_then(|u| u.looking_for.clone());

        let preferred_professions: Vec<String> = preferred_professions
            .flatten()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();

        // RL-based ranking: score candidates and re-sort (with 2s timeout + graceful fallback)
        let candidate_ids: Vec<i32> = rows.iter().map(|r| r.id as i32).collect();
        let ml_scores: std::collections::HashMap<i32, f64> = match tokio::time::timeout(
            std::time::Duration::from_secs(2),
            async {
                let mut ml = state.ml.write().await;
                ml.rank_candidates(&state.db, user_id as i32, &candidate_ids).await
            },
        ).await {
            Ok(scores) => scores.into_iter().collect(),
            Err(_) => {
                tracing::warn!(user_id, "ML ranking timed out in GraphQL discover, using basic scoring");
                std::collections::HashMap::new()
            }
        };

        // Graph features: shared likes + shared reel views per candidate (batch query)
        let uid_str = user_id.to_string();
        let graph_scores: std::collections::HashMap<i64, f64> = {
            let candidate_id_strs: Vec<String> = rows.iter().map(|r| r.id.to_string()).collect();
            if candidate_id_strs.is_empty() {
                std::collections::HashMap::new()
            } else {
                // Count shared "liked" edges (users we both liked)
                let shared_likes = sqlx::query_as::<_, (String, i64)>(
                    r#"SELECT f2.from_id, COUNT(*) as shared
                       FROM graph_edge_links_fwd f1
                       JOIN graph_edge_links_fwd f2 ON f1.to_id = f2.to_id AND f1.edge_type = f2.edge_type
                       WHERE f1.from_type = 'user' AND f1.from_id = $1
                         AND f1.edge_type = 'liked' AND f2.from_type = 'user'
                         AND f2.from_id = ANY($2) AND f2.from_id != $1
                       GROUP BY f2.from_id"#,
                )
                .bind(&uid_str)
                .bind(&candidate_id_strs)
                .fetch_all(&state.db)
                .await
                .unwrap_or_default();

                // Count shared reel views
                let shared_views = sqlx::query_as::<_, (String, i64)>(
                    r#"SELECT f2.from_id, COUNT(*) as shared
                       FROM graph_edge_links_fwd f1
                       JOIN graph_edge_links_fwd f2 ON f1.to_id = f2.to_id AND f1.edge_type = f2.edge_type
                       WHERE f1.from_type = 'user' AND f1.from_id = $1
                         AND f1.edge_type = 'viewed' AND f2.from_type = 'user'
                         AND f2.from_id = ANY($2) AND f2.from_id != $1
                       GROUP BY f2.from_id"#,
                )
                .bind(&uid_str)
                .bind(&candidate_id_strs)
                .fetch_all(&state.db)
                .await
                .unwrap_or_default();

                let mut scores = std::collections::HashMap::new();
                for (cid, count) in shared_likes {
                    if let Ok(id) = cid.parse::<i64>() {
                        *scores.entry(id).or_insert(0.0) += count as f64 * 0.15; // 15% weight per shared like
                    }
                }
                for (cid, count) in shared_views {
                    if let Ok(id) = cid.parse::<i64>() {
                        *scores.entry(id).or_insert(0.0) += count as f64 * 0.05; // 5% weight per shared view
                    }
                }
                // Normalize to 0-1 range
                let max_val = scores.values().cloned().fold(0.0f64, f64::max).max(1.0);
                for v in scores.values_mut() { *v /= max_val; }
                scores
            }
        };

        // Music compatibility: genre overlap per candidate (batch query)
        let music_scores: std::collections::HashMap<i64, f64> = {
            let candidate_i64s: Vec<i64> = rows.iter().map(|r| r.id).collect();
            let shared_genres = sqlx::query_as::<_, (i64, i64)>(
                r#"SELECT b.user_id, COUNT(*) as shared
                   FROM user_genre_profile a
                   JOIN user_genre_profile b ON a.genre = b.genre
                   WHERE a.user_id = $1 AND b.user_id = ANY($2)
                   GROUP BY b.user_id"#,
            )
            .bind(user_id)
            .bind(&candidate_i64s)
            .fetch_all(&state.db)
            .await
            .unwrap_or_default();

            // Get user's total genres for Jaccard denominator
            let my_genre_count = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM user_genre_profile WHERE user_id = $1"
            ).bind(user_id).fetch_one(&state.db).await.unwrap_or(1);

            shared_genres.into_iter().map(|(cid, shared)| {
                let score = (shared as f64 / my_genre_count.max(1) as f64).min(1.0);
                (cid, score)
            }).collect()
        };

        // Combine: ML (55%) + graph (15%) + music (10%) + basic (20%)
        let mut ranked_rows: Vec<_> = rows.into_iter().map(|r| {
            let ml_score = ml_scores.get(&(r.id as i32)).copied().unwrap_or(0.0);
            let graph_score = graph_scores.get(&r.id).copied().unwrap_or(0.0);
            let music_score = music_scores.get(&r.id).copied().unwrap_or(0.0);
            let combined = 0.55 * ml_score + 0.15 * graph_score + 0.10 * music_score + 0.20 * ml_score.max(0.5);
            (combined, ml_score, graph_score, music_score, r)
        }).collect();

        ranked_rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        Ok(ranked_rows.into_iter().map(|(combined_score, ml_score, graph_score, music_score, r)| {
            let age = r.dob.map(|dob| {
                let today = chrono::Utc::now().date_naive();
                let mut age = today.year() - dob.year();
                if today.ordinal() < dob.ordinal() {
                    age -= 1;
                }
                age
            });

            let photos: Vec<String> = r.profile_photos
                .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
                .unwrap_or_else(|| {
                    [r.profile_photo_1, r.profile_photo_2, r.profile_photo_3]
                        .into_iter()
                        .flatten()
                        .collect()
                });

            let interests: Vec<String> = r.interests
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default();

            let languages: Vec<String> = r.languages
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default();

            // Calculate compatibility: ML (55%) + graph (15%) + music (10%) + basic (20%)
            let basic_score = calculate_compatibility(
                &current_interests,
                &current_languages,
                &current_looking_for,
                &preferred_professions,
                &interests,
                &languages,
                &r.looking_for,
                &r.profession_category,
                r.voice_intro_url.is_some(),
                r.is_verified.unwrap_or(false),
            );
            let compatibility_score = if combined_score > 0.0 {
                let blended = 0.55 * ml_score * 100.0
                    + 0.15 * graph_score * 100.0
                    + 0.10 * music_score * 100.0
                    + 0.20 * basic_score;
                blended.min(100.0)
            } else {
                basic_score
            };

            let has_voice = r.voice_intro_url.is_some();

            DiscoverProfile {
                id: r.id,
                name: r.name,
                age,
                gender: r.gender,
                bio: r.bio,
                location: r.location_text,
                photos,
                interests,
                compatibility_score: Some(compatibility_score),
                distance_km: None,
                is_verified: r.is_verified.unwrap_or(false),
                voice_intro_url: r.voice_intro_url,
                voice_intro_duration: r.voice_intro_duration,
                has_voice_intro: has_voice,
                profession_title: r.profession_title,
                languages,
                looking_for: r.looking_for,
                height_cm: r.height_cm,
            }
        }).collect())
    }

    /// Get matches for current user (paginated)
    async fn matches(
        &self,
        ctx: &Context<'_>,
        #[graphql(default = 30)] limit: Option<i32>,
    ) -> Result<Vec<Match>> {
        let state = ctx.data::<AppState>()?;
        let user_id = get_user_id_from_context(ctx)?;
        let loader = ctx.data::<DataLoader<UserLoader>>()?;

        let effective_limit: i64 = (limit.unwrap_or(30) as i64).min(100).max(1);

        let rows = sqlx::query_as::<_, MatchRow>(
            r#"
            SELECT m.id, m.user1_id, m.user2_id, m.is_mutual_match, m.status,
                   m.created_at as matched_at,
                   CASE WHEN s.action = 'super_like' THEN 'super_like' ELSE 'swipe' END AS like_type,
                   msg.content AS message
            FROM matches m
            LEFT JOIN swipes s ON (
                (s.from_user_id = m.user1_id AND s.to_user_id = m.user2_id)
                OR (s.from_user_id = m.user2_id AND s.to_user_id = m.user1_id)
            ) AND s.action IN ('like', 'super_like')
            LEFT JOIN messages msg ON msg.match_id = m.id AND msg.message_type = 'like_message'
                AND msg.sender_id != $1
            WHERE (m.user1_id = $1 OR m.user2_id = $1) AND m.is_mutual_match = true
            ORDER BY m.created_at DESC
            LIMIT $2
            "#,
        )
        .bind(user_id)
        .bind(effective_limit)
        .fetch_all(&state.db)
        .await?;

        let mut matches = Vec::new();
        for row in rows {
            let partner_id = if row.user1_id == user_id { row.user2_id } else { row.user1_id };
            let partner = loader.load_one(partner_id).await?;

            matches.push(Match {
                id: row.id.clone(),
                user1_id: i64::from(row.user1_id),
                user2_id: i64::from(row.user2_id),
                is_mutual: row.is_mutual_match.unwrap_or(false),
                status: row.status,
                matched_at: row.matched_at.map(|dt| dt.to_string()),
                partner,
                like_type: row.like_type.or(Some("swipe".to_string())),
                message: row.message,
            });
        }

        Ok(matches)
    }

    /// Get likes the current user has sent (pending, not yet matched)
    #[graphql(name = "sentLikes")]
    async fn sent_likes(&self, ctx: &Context<'_>) -> Result<Vec<LikedProfile>> {
        let state = ctx.data::<AppState>()?;
        let user_id = get_user_id_from_context(ctx)?;

        let rows = sqlx::query_as::<_, LikedProfileRow>(r#"
            SELECT u.id, u.name, u.dob, u.profile_photo_1, u.profile_photo_url,
                   u.profile_photos, u.university, u.profession_title,
                   s.action as like_type, s.created_at as liked_at,
                   NULL::text as message
            FROM swipes s
            JOIN users u ON u.id = s.to_user_id::int
            WHERE s.from_user_id = $1
              AND s.action IN ('like', 'super_like')
              AND NOT EXISTS (
                  SELECT 1 FROM matches m
                  WHERE (m.user1_id = $1 AND m.user2_id = s.to_user_id::int)
                     OR (m.user2_id = $1 AND m.user1_id = s.to_user_id::int)
              )
            ORDER BY s.created_at DESC
            LIMIT 100
        "#)
        .bind(user_id)
        .fetch_all(&state.db)
        .await?;

        Ok(rows.into_iter().map(liked_profile_row_to_type).collect())
    }

    /// Get likes received from other users (pending message requests + swipe likes)
    #[graphql(name = "receivedLikes")]
    async fn received_likes(&self, ctx: &Context<'_>) -> Result<Vec<LikedProfile>> {
        let state = ctx.data::<AppState>()?;
        let user_id = get_user_id_from_context(ctx)?;

        let rows = sqlx::query_as::<_, LikedProfileRow>(r#"
            SELECT u.id, u.name, u.dob, u.profile_photo_1, u.profile_photo_url,
                   u.profile_photos, u.university, u.profession_title,
                   s.action as like_type, s.created_at as liked_at,
                   msg.content as message
            FROM swipes s
            JOIN users u ON u.id = s.from_user_id::int
            LEFT JOIN messages msg ON msg.sender_id = s.from_user_id::int
                AND msg.receiver_id = $1
                AND msg.message_type = 'like_message'
            WHERE s.to_user_id = $1
              AND s.action IN ('like', 'super_like')
              AND NOT EXISTS (
                  SELECT 1 FROM matches m
                  WHERE (m.user1_id = $1 AND m.user2_id = s.from_user_id::int)
                     OR (m.user2_id = $1 AND m.user1_id = s.from_user_id::int)
              )
            ORDER BY s.action DESC, s.created_at DESC
            LIMIT 100
        "#)
        .bind(user_id)
        .fetch_all(&state.db)
        .await?;

        Ok(rows.into_iter().map(liked_profile_row_to_type).collect())
    }

    /// Get conversation messages for a match
    /// Pass `since` (ISO 8601 timestamp) to get only messages after that time (for offline sync)
    async fn conversation(
        &self,
        ctx: &Context<'_>,
        match_id: String,
        limit: Option<i32>,
        offset: Option<i32>,
        since: Option<String>,
    ) -> Result<Vec<Message>> {
        let state = ctx.data::<AppState>()?;
        let user_id = get_user_id_from_context(ctx)?;
        // Verify user is part of this match (match_id is VARCHAR, not UUID)
        let match_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM matches WHERE id = $1 AND (user1_id = $2 OR user2_id = $2)",
        )
        .bind(&match_id)
        .bind(user_id)
        .fetch_one(&state.db)
        .await?;

        if match_count == 0 {
            return Err(Error::new("Not authorized to view this conversation"));
        }

        let since_ts = since.as_deref()
            .and_then(|s| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").ok()
                .or_else(|| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok()));

        let rows = if let Some(ts) = since_ts {
            sqlx::query_as::<_, MessageRow>(
                r#"
                SELECT id, match_id, sender_id, receiver_id, content, created_at, is_read
                FROM messages
                WHERE match_id = $1 AND created_at > $2
                ORDER BY created_at ASC
                LIMIT $3
                "#,
            )
            .bind(&match_id)
            .bind(ts)
            .bind(limit.unwrap_or(500))
            .fetch_all(&state.db)
            .await?
        } else {
            sqlx::query_as::<_, MessageRow>(
                r#"
                SELECT id, match_id, sender_id, receiver_id, content, created_at, is_read
                FROM messages
                WHERE match_id = $1
                ORDER BY created_at ASC
                LIMIT $2 OFFSET $3
                "#,
            )
            .bind(&match_id)
            .bind(limit.unwrap_or(100))
            .bind(offset.unwrap_or(0))
            .fetch_all(&state.db)
            .await?
        };

        Ok(rows.into_iter().map(|r| Message {
            id: r.id,
            match_id: r.match_id,
            sender_id: i64::from(r.sender_id),
            receiver_id: i64::from(r.receiver_id),
            content: r.content,
            created_at: r.created_at.map(|dt| dt.to_string()),
            is_read: r.is_read.unwrap_or(false),
        }).collect())
    }

    /// Search universities by name (autocomplete)
    #[graphql(name = "searchUniversities")]
    async fn search_universities(
        &self,
        ctx: &Context<'_>,
        query: String,
        limit: Option<i32>,
    ) -> Result<Vec<UniversityResult>> {
        let state = ctx.data::<AppState>()?;
        let _user_id = get_user_id_from_context(ctx)?;

        let search = format!("%{}%", query.trim());
        let lim = limit.unwrap_or(10).min(50);

        let rows = sqlx::query_as::<_, UniversitySearchRow>(
            r#"
            SELECT u.id, u.name, u.domain, u.country, u.tier,
                   COUNT(sv.id) as student_count
            FROM universities u
            LEFT JOIN student_verifications sv ON sv.university_name = u.name AND sv.status = 'approved'
            WHERE u.name ILIKE $1
            GROUP BY u.id, u.name, u.domain, u.country, u.tier
            ORDER BY COUNT(sv.id) DESC, u.name
            LIMIT $2
            "#,
        )
        .bind(&search)
        .bind(lim)
        .fetch_all(&state.db)
        .await?;

        Ok(rows.into_iter().map(|r| UniversityResult {
            id: r.id,
            name: r.name,
            domain: r.domain,
            country: r.country,
            tier: r.tier,
            student_count: r.student_count.unwrap_or(0),
        }).collect())
    }

    /// Get profiles from a specific university with optional gender filter
    #[graphql(name = "universityProfiles")]
    async fn university_profiles(
        &self,
        ctx: &Context<'_>,
        university_id: i64,
        gender: Option<String>,
        limit: Option<i32>,
    ) -> Result<UniversityProfileResult> {
        let state = ctx.data::<AppState>()?;
        let _user_id = get_user_id_from_context(ctx)?;

        let lim = limit.unwrap_or(50).min(100);

        // Get university info
        let uni = sqlx::query_as::<_, UniversityInfoRow>(
            "SELECT id, name, domain, country, tier FROM universities WHERE id = $1",
        )
        .bind(university_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| Error::new("University not found"))?;

        let gender_filter = gender.as_deref().filter(|g| !g.trim().is_empty());

        let (profiles, total) = if let Some(g) = gender_filter {
            let rows = sqlx::query_as::<_, DiscoverRow>(
                r#"
                SELECT u.id, u.name, u.dob, u.gender, u.bio, u.location_text,
                       u.profile_photo_1, u.profile_photo_2, u.profile_photo_3, u.profile_photos,
                       u.interests, u.languages, u.is_verified, u.attractiveness_score,
                       u.voice_intro_url, u.voice_intro_duration,
                       u.profession_title, u.profession_category, u.looking_for, u.height_cm
                FROM users u
                JOIN student_verifications sv ON sv.user_id = u.id AND sv.status = 'approved'
                WHERE sv.university_name = $1
                  AND u.is_profile_complete = true AND u.is_active = true
                  AND LOWER(u.gender) = LOWER($2)
                ORDER BY u.attractiveness_score DESC NULLS LAST
                LIMIT $3
                "#,
            )
            .bind(&uni.name)
            .bind(g)
            .bind(lim)
            .fetch_all(&state.db)
            .await?;

            let count = sqlx::query_scalar::<_, i64>(
                r#"
                SELECT COUNT(*) FROM users u
                JOIN student_verifications sv ON sv.user_id = u.id AND sv.status = 'approved'
                WHERE sv.university_name = $1 AND u.is_profile_complete = true AND u.is_active = true
                  AND LOWER(u.gender) = LOWER($2)
                "#,
            )
            .bind(&uni.name)
            .bind(g)
            .fetch_one(&state.db)
            .await?;

            (rows, count)
        } else {
            let rows = sqlx::query_as::<_, DiscoverRow>(
                r#"
                SELECT u.id, u.name, u.dob, u.gender, u.bio, u.location_text,
                       u.profile_photo_1, u.profile_photo_2, u.profile_photo_3, u.profile_photos,
                       u.interests, u.languages, u.is_verified, u.attractiveness_score,
                       u.voice_intro_url, u.voice_intro_duration,
                       u.profession_title, u.profession_category, u.looking_for, u.height_cm
                FROM users u
                JOIN student_verifications sv ON sv.user_id = u.id AND sv.status = 'approved'
                WHERE sv.university_name = $1
                  AND u.is_profile_complete = true AND u.is_active = true
                ORDER BY u.attractiveness_score DESC NULLS LAST
                LIMIT $2
                "#,
            )
            .bind(&uni.name)
            .bind(lim)
            .fetch_all(&state.db)
            .await?;

            let count = sqlx::query_scalar::<_, i64>(
                r#"
                SELECT COUNT(*) FROM users u
                JOIN student_verifications sv ON sv.user_id = u.id AND sv.status = 'approved'
                WHERE sv.university_name = $1 AND u.is_profile_complete = true AND u.is_active = true
                "#,
            )
            .bind(&uni.name)
            .fetch_one(&state.db)
            .await?;

            (rows, count)
        };

        let discover_profiles: Vec<DiscoverProfile> = profiles.into_iter().map(|r| {
            discover_row_to_profile(r)
        }).collect();

        Ok(UniversityProfileResult {
            university: UniversityInfo {
                id: uni.id,
                name: uni.name,
                domain: uni.domain,
                country: uni.country,
                tier: uni.tier,
            },
            profiles: discover_profiles,
            total,
        })
    }

    /// Unified search across universities and profiles
    #[graphql(name = "unifiedSearch")]
    async fn unified_search(
        &self,
        ctx: &Context<'_>,
        query: String,
        gender: Option<String>,
        limit: Option<i32>,
    ) -> Result<UnifiedSearchResult> {
        let state = ctx.data::<AppState>()?;
        let _user_id = get_user_id_from_context(ctx)?;

        let search = format!("%{}%", query.trim());
        let lim = limit.unwrap_or(20).min(50);

        // Search universities
        let uni_rows = sqlx::query_as::<_, UniversitySearchRow>(
            r#"
            SELECT u.id, u.name, u.domain, u.country, u.tier,
                   COUNT(sv.id) as student_count
            FROM universities u
            LEFT JOIN student_verifications sv ON sv.university_name = u.name AND sv.status = 'approved'
            WHERE u.name ILIKE $1
            GROUP BY u.id, u.name, u.domain, u.country, u.tier
            ORDER BY COUNT(sv.id) DESC, u.name
            LIMIT $2
            "#,
        )
        .bind(&search)
        .bind(lim)
        .fetch_all(&state.db)
        .await?;

        let universities: Vec<UniversityResult> = uni_rows.into_iter().map(|r| UniversityResult {
            id: r.id,
            name: r.name,
            domain: r.domain,
            country: r.country,
            tier: r.tier,
            student_count: r.student_count.unwrap_or(0),
        }).collect();

        // Search profiles by name
        let gender_filter = gender.as_deref().filter(|g| !g.trim().is_empty());

        let profile_rows = if let Some(g) = gender_filter {
            sqlx::query_as::<_, DiscoverRow>(
                r#"
                SELECT u.id, u.name, u.dob, u.gender, u.bio, u.location_text,
                       u.profile_photo_1, u.profile_photo_2, u.profile_photo_3, u.profile_photos,
                       u.interests, u.languages, u.is_verified, u.attractiveness_score,
                       u.voice_intro_url, u.voice_intro_duration,
                       u.profession_title, u.profession_category, u.looking_for, u.height_cm
                FROM users u
                WHERE u.name ILIKE $1
                  AND u.is_profile_complete = true AND u.is_active = true
                  AND LOWER(u.gender) = LOWER($2)
                ORDER BY u.attractiveness_score DESC NULLS LAST
                LIMIT $3
                "#,
            )
            .bind(&search)
            .bind(g)
            .bind(lim)
            .fetch_all(&state.db)
            .await?
        } else {
            sqlx::query_as::<_, DiscoverRow>(
                r#"
                SELECT u.id, u.name, u.dob, u.gender, u.bio, u.location_text,
                       u.profile_photo_1, u.profile_photo_2, u.profile_photo_3, u.profile_photos,
                       u.interests, u.languages, u.is_verified, u.attractiveness_score,
                       u.voice_intro_url, u.voice_intro_duration,
                       u.profession_title, u.profession_category, u.looking_for, u.height_cm
                FROM users u
                WHERE u.name ILIKE $1
                  AND u.is_profile_complete = true AND u.is_active = true
                ORDER BY u.attractiveness_score DESC NULLS LAST
                LIMIT $2
                "#,
            )
            .bind(&search)
            .bind(lim)
            .fetch_all(&state.db)
            .await?
        };

        let profiles: Vec<DiscoverProfile> = profile_rows.into_iter().map(|r| {
            discover_row_to_profile(r)
        }).collect();

        Ok(UnifiedSearchResult {
            universities,
            profiles,
        })
    }

    /// Get student verification status
    async fn student_status(&self, ctx: &Context<'_>) -> Result<StudentStatus> {
        let state = ctx.data::<AppState>()?;
        let user_id = get_user_id_from_context(ctx)?;

        let row = sqlx::query_as::<_, StudentVerificationRow>(
            r#"
            SELECT university_name, discount_tier, expires_at
            FROM student_verifications
            WHERE user_id = $1 AND status = 'approved'
            ORDER BY verified_at DESC LIMIT 1
            "#,
        )
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?;

        match row {
            Some(r) => {
                let discount_percent = match r.discount_tier.as_deref() {
                    Some("top_private") => (state.config.student_discount_ivy * 100.0) as i32,
                    Some("top_public") => (state.config.student_discount_top50 * 100.0) as i32,
                    Some("graduate") => (state.config.student_discount_graduate * 100.0) as i32,
                    Some("alumni") => (state.config.student_discount_alumni * 100.0) as i32,
                    _ => (state.config.student_discount_other * 100.0) as i32,
                };

                Ok(StudentStatus {
                    is_verified: true,
                    university_name: r.university_name,
                    discount_tier: r.discount_tier,
                    discount_percent,
                    expires_at: r.expires_at.map(|dt| dt.to_string()),
                })
            }
            None => Ok(StudentStatus {
                is_verified: false,
                university_name: None,
                discount_tier: None,
                discount_percent: 0,
                expires_at: None,
            }),
        }
    }
}

// ============================================================================
// Mutation Root
// ============================================================================

pub struct MutationRoot;

#[Object]
impl MutationRoot {
    /// Send OTP to phone number
    async fn send_otp(&self, _ctx: &Context<'_>, phone_number: String) -> Result<OtpResponse> {
        if phone_number.trim().is_empty() {
            return Err(Error::new("Phone number is required"));
        }

        // In production, integrate with SMS provider
        // For development, return the test OTP
        Ok(OtpResponse {
            message: "OTP sent successfully".to_string(),
            otp: Some("1234".to_string()), // Only in dev
        })
    }

    /// Verify OTP and get auth token
    async fn verify_otp(
        &self,
        ctx: &Context<'_>,
        phone_number: String,
        otp: String,
    ) -> Result<AuthPayload> {
        let state = ctx.data::<AppState>()?;

        if phone_number.trim().is_empty() || otp.trim().is_empty() {
            return Err(Error::new("Phone number and OTP are required"));
        }

        // Verify OTP (hardcoded for development)
        if otp != "1234" {
            return Err(Error::new("Invalid OTP"));
        }

        // Check if user exists
        let existing = sqlx::query_scalar::<_, i64>(
            "SELECT id FROM users WHERE phone_number = $1",
        )
        .bind(&phone_number)
        .fetch_optional(&state.db)
        .await?;

        let (user_id, is_new_user, is_profile_complete) = match existing {
            Some(id) => {
                let complete = sqlx::query_scalar::<_, Option<bool>>(
                    "SELECT is_profile_complete FROM users WHERE id = $1",
                )
                .bind(id)
                .fetch_one(&state.db)
                .await?
                .unwrap_or(false);

                (id, false, complete)
            }
            None => {
                let id = sqlx::query_scalar::<_, i64>(
                    r#"
                    INSERT INTO users (phone_number, is_active, is_profile_complete, created_at, updated_at)
                    VALUES ($1, TRUE, FALSE, NOW(), NOW())
                    RETURNING id
                    "#,
                )
                .bind(&phone_number)
                .fetch_one(&state.db)
                .await?;

                (id, true, false)
            }
        };

        // Update last_active
        let _ = sqlx::query("UPDATE users SET last_active = NOW() WHERE id = $1")
            .bind(user_id)
            .execute(&state.db)
            .await;

        // Create JWT
        let token = create_access_token(
            user_id,
            &state.config.secret_key,
            state.config.access_token_expire_minutes,
        )?;

        Ok(AuthPayload {
            access_token: token,
            user_id,
            is_new_user,
            is_profile_complete,
        })
    }

    /// Update user profile - accepts flat arguments for frontend compatibility
    #[graphql(name = "update_profile")]
    async fn update_profile(
        &self,
        ctx: &Context<'_>,
        name: Option<String>,
        #[graphql(name = "display_name")] display_name: Option<String>,
        gender: Option<String>,
        dob: Option<String>,
        bio: Option<String>,
        location: Option<String>,
        interests: Option<Vec<String>>,
        languages: Option<Vec<String>>,
        #[graphql(name = "looking_for")] looking_for: Option<String>,
        #[graphql(name = "profession_category")] profession_category: Option<String>,
        #[graphql(name = "profession_title")] profession_title: Option<String>,
        #[graphql(name = "height_cm")] height_cm: Option<i32>,
        university: Option<String>,
        #[graphql(name = "university_location")] university_location: Option<String>,
        study: Option<String>,
        #[graphql(name = "profile_photo_1")] profile_photo_1: Option<String>,
        #[graphql(name = "profile_photo_2")] profile_photo_2: Option<String>,
        #[graphql(name = "profile_photo_3")] profile_photo_3: Option<String>,
        #[graphql(default)] photos: Vec<Upload>,
    ) -> Result<bool> {
        let state = ctx.data::<AppState>()?;
        let user_id = get_user_id_from_context(ctx)?;

        // Lock immutable fields: gender and dob cannot be changed once set
        if gender.is_some() || dob.is_some() {
            #[derive(sqlx::FromRow)]
            struct LockedFields { gender: Option<String>, dob: Option<chrono::NaiveDate> }
            let existing = sqlx::query_as::<_, LockedFields>(
                "SELECT gender, dob FROM users WHERE id = $1"
            ).bind(user_id).fetch_optional(&state.db).await?;
            if let Some(ref row) = existing {
                if gender.is_some() && row.gender.is_some() {
                    return Err(Error::new("Gender cannot be changed after initial setup"));
                }
                if dob.is_some() && row.dob.is_some() {
                    return Err(Error::new("Date of birth cannot be changed after initial setup"));
                }
            }
        }

        // Build dynamic update query
        let mut updates = Vec::new();
        let mut params: Vec<String> = Vec::new();

        // Identity lock: once student-verified, users.name is immutable.
        // A verified user's `name` argument is redirected into display_name.
        let (is_verified, current_name) = {
            #[derive(sqlx::FromRow)]
            struct VRow { is_student_verified: Option<bool>, name: Option<String> }
            match sqlx::query_as::<_, VRow>(
                "SELECT is_student_verified, name FROM users WHERE id = $1"
            ).bind(user_id).fetch_optional(&state.db).await? {
                Some(r) => (r.is_student_verified.unwrap_or(false), r.name),
                None => (false, None),
            }
        };

        let (effective_name, effective_display_name) = if is_verified {
            // Verified: reject any name change, route new value to display_name.
            let forced_display = match (&name, &current_name) {
                (Some(n), Some(cn)) if n != cn => Some(n.clone()),
                _ => None,
            };
            (None, display_name.clone().or(forced_display))
        } else {
            (name.clone(), display_name.clone())
        };

        if let Some(ref val) = effective_name {
            updates.push(format!("name = ${}", params.len() + 2));
            params.push(val.clone());
        }
        if let Some(ref val) = effective_display_name {
            updates.push(format!("display_name = ${}", params.len() + 2));
            params.push(val.clone());
        }
        if let Some(ref val) = gender {
            updates.push(format!("gender = ${}", params.len() + 2));
            params.push(val.clone());
        }
        // dob handled separately as DATE type
        if let Some(ref val) = bio {
            updates.push(format!("bio = ${}", params.len() + 2));
            params.push(val.clone());
        }
        if let Some(ref val) = location {
            updates.push(format!("location_text = ${}", params.len() + 2));
            params.push(val.clone());
        }
        if let Some(ref val) = looking_for {
            updates.push(format!("looking_for = ${}", params.len() + 2));
            params.push(val.clone());
        }
        if let Some(ref val) = profession_category {
            updates.push(format!("profession_category = ${}", params.len() + 2));
            params.push(val.clone());
        }
        if let Some(ref val) = profession_title {
            updates.push(format!("profession_title = ${}", params.len() + 2));
            params.push(val.clone());
        }
        if let Some(ref val) = university {
            updates.push(format!("university = ${}", params.len() + 2));
            params.push(val.clone());
        }
        // university_location and study are not users table columns — ignored here

        updates.push("updated_at = NOW()".to_string());

        if updates.len() > 1 {
            let query = format!(
                "UPDATE users SET {} WHERE id = $1",
                updates.join(", ")
            );

            let mut q = sqlx::query(&query).bind(user_id);
            for p in &params {
                q = q.bind(p);
            }
            q.execute(&state.db).await?;
        }

        // Handle dob separately (DATE type) - parse from string like "1990-01-15" or "January 15, 1990"
        if let Some(ref dob_str) = dob {
            // Try parsing as YYYY-MM-DD first
            if let Ok(date) = chrono::NaiveDate::parse_from_str(dob_str, "%Y-%m-%d") {
                sqlx::query("UPDATE users SET dob = $1 WHERE id = $2")
                    .bind(date)
                    .bind(user_id)
                    .execute(&state.db)
                    .await?;
            } else if let Ok(date) = chrono::NaiveDate::parse_from_str(dob_str, "%B %d, %Y") {
                // Try "January 15, 1990" format
                sqlx::query("UPDATE users SET dob = $1 WHERE id = $2")
                    .bind(date)
                    .bind(user_id)
                    .execute(&state.db)
                    .await?;
            } else if let Ok(date) = chrono::NaiveDate::parse_from_str(dob_str, "%m/%d/%Y") {
                // Try MM/DD/YYYY format
                sqlx::query("UPDATE users SET dob = $1 WHERE id = $2")
                    .bind(date)
                    .bind(user_id)
                    .execute(&state.db)
                    .await?;
            }
            // If parsing fails, skip dob update silently
        }

        // Handle height_cm separately (integer)
        if let Some(height) = height_cm {
            sqlx::query("UPDATE users SET height_cm = $1 WHERE id = $2")
                .bind(height)
                .bind(user_id)
                .execute(&state.db)
                .await?;
        }

        // profile_photo_N accepts two forms:
        // - URL paths ("/uploads/..." or "https://...") — kept as-is (a photo
        //   the user didn't change this round)
        // - base64 data URIs ("data:image/jpeg;base64,...") — actual photo
        //   uploads from the client: decoded and run through the same
        //   pipeline + object storage as file uploads, slot-preserving.
        //   (These were previously dropped silently, losing the photos.)
        // Placeholder strings (starting with #) are ignored.
        let is_valid_photo_url = |p: &str| -> bool {
            !p.is_empty()
                && !p.starts_with('#')
                && !p.starts_with("data:")
                && (p.starts_with('/') || p.starts_with("http"))
        };
        let clean_photo_1 = profile_photo_1.clone().filter(|p| is_valid_photo_url(p));
        let clean_photo_2 = profile_photo_2.clone().filter(|p| is_valid_photo_url(p));
        let clean_photo_3 = profile_photo_3.clone().filter(|p| is_valid_photo_url(p));

        // Decode + store base64 data URIs per slot. Failures are loud (GraphQL
        // errors), not silent drops — the client must know its photo was lost.
        let mut slot_urls: [Option<String>; 3] = [None, None, None];
        for (idx, raw) in [&profile_photo_1, &profile_photo_2, &profile_photo_3]
            .into_iter()
            .enumerate()
        {
            let Some(s) = raw.as_deref() else { continue };
            if !s.starts_with("data:") {
                continue;
            }
            let b64 = s.split_once(',').map(|(_, tail)| tail).unwrap_or("");
            let bytes = {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .map_err(|_| Error::new(format!("Photo {}: invalid base64 data URI", idx + 1)))?
            };
            if bytes.len() > state.config.max_photo_bytes {
                return Err(Error::new(format!(
                    "Photo {} exceeds the {}MB limit",
                    idx + 1,
                    state.config.max_photo_bytes / 1024 / 1024
                )));
            }
            slot_urls[idx] =
                process_and_store_profile_photo(state, user_id, idx, bytes).await?;
        }
        let [slot_url_1, slot_url_2, slot_url_3] = slot_urls;

        // Handle uploaded photo files — run full pipeline (resize, NSFW, EXIF strip, quality)
        let mut uploaded_urls: Vec<String> = Vec::new();

        for (slot_idx, upload) in photos.into_iter().enumerate() {
            let upload_value = upload.value(ctx)?;

            // Read bytes
            let mut reader = upload_value.into_read();
            let mut content = Vec::new();
            std::io::Read::read_to_end(&mut reader, &mut content).ok();

            if let Some(url) =
                process_and_store_profile_photo(state, user_id, slot_idx, content).await?
            {
                uploaded_urls.push(url);
            }
        }

        // Build photo array maintaining position order:
        // - If clean_photo_N is a valid URL string, use it
        // - Else if the slot carried a data URI, use its freshly stored URL
        // - Else if we have file uploads, use the next upload for that position
        // This allows users to replace individual photos while keeping others
        let mut upload_iter = uploaded_urls.into_iter();
        let final_photo_1 = clean_photo_1.clone().or(slot_url_1).or_else(|| upload_iter.next());
        let final_photo_2 = clean_photo_2.clone().or(slot_url_2).or_else(|| upload_iter.next());
        let final_photo_3 = clean_photo_3.clone().or(slot_url_3).or_else(|| upload_iter.next());

        // Collect non-None photos in order
        let mut all_photos: Vec<String> = Vec::new();
        if let Some(p) = &final_photo_1 { all_photos.push(p.clone()); }
        if let Some(p) = &final_photo_2 { all_photos.push(p.clone()); }
        if let Some(p) = &final_photo_3 { all_photos.push(p.clone()); }
        // Add any remaining uploads beyond position 3
        all_photos.extend(upload_iter);

        let photos_to_save = if !all_photos.is_empty() {
            Some(all_photos)
        } else {
            None
        };

        if let Some(ref photos) = photos_to_save {
            // Store in JSONB for new format
            let json = serde_json::to_value(photos)?;
            sqlx::query("UPDATE users SET profile_photos = $1 WHERE id = $2")
                .bind(&json)
                .bind(user_id)
                .execute(&state.db)
                .await?;
        }

        // Update individual columns with final values
        if final_photo_1.is_some() {
            sqlx::query("UPDATE users SET profile_photo_1 = $1 WHERE id = $2")
                .bind(&final_photo_1)
                .bind(user_id)
                .execute(&state.db)
                .await?;
        }
        if final_photo_2.is_some() {
            sqlx::query("UPDATE users SET profile_photo_2 = $1 WHERE id = $2")
                .bind(&final_photo_2)
                .bind(user_id)
                .execute(&state.db)
                .await?;
        }
        if final_photo_3.is_some() {
            sqlx::query("UPDATE users SET profile_photo_3 = $1 WHERE id = $2")
                .bind(&final_photo_3)
                .bind(user_id)
                .execute(&state.db)
                .await?;
        }

        // Update interests if provided
        if let Some(ref int) = interests {
            let json = serde_json::to_value(int)?;
            sqlx::query("UPDATE users SET interests = $1 WHERE id = $2")
                .bind(json)
                .bind(user_id)
                .execute(&state.db)
                .await?;
        }

        // Update languages if provided
        if let Some(ref lang) = languages {
            let json = serde_json::to_value(lang)?;
            sqlx::query("UPDATE users SET languages = $1 WHERE id = $2")
                .bind(json)
                .bind(user_id)
                .execute(&state.db)
                .await?;
        }

        // Check and update profile completeness
        let _ = sqlx::query(
            r#"
            UPDATE users SET is_profile_complete = TRUE
            WHERE id = $1 AND name IS NOT NULL AND gender IS NOT NULL
            "#,
        )
        .bind(user_id)
        .execute(&state.db)
        .await;

        // Return success
        Ok(true)
    }

    /// Update user preferences
    async fn save_preferences(&self, ctx: &Context<'_>, input: PreferencesInput) -> Result<UserPreferences> {
        let state = ctx.data::<AppState>()?;
        let user_id = get_user_id_from_context(ctx)?;

        let preferred_genders = input.preferred_genders
            .map(|g| serde_json::to_value(g).unwrap_or_default());

        let preferred_professions = input.preferred_professions
            .map(|p| serde_json::to_value(p).unwrap_or_default());

        sqlx::query(
            r#"
            INSERT INTO user_preferences (user_id, min_age, max_age, preferred_genders, preferred_professions, max_distance, only_verified, only_students, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())
            ON CONFLICT (user_id) DO UPDATE SET
                min_age = COALESCE($2, user_preferences.min_age),
                max_age = COALESCE($3, user_preferences.max_age),
                preferred_genders = COALESCE($4, user_preferences.preferred_genders),
                preferred_professions = COALESCE($5, user_preferences.preferred_professions),
                max_distance = COALESCE($6, user_preferences.max_distance),
                only_verified = COALESCE($7, user_preferences.only_verified),
                only_students = COALESCE($8, user_preferences.only_students),
                updated_at = NOW()
            "#,
        )
        .bind(user_id)
        .bind(input.min_age)
        .bind(input.max_age)
        .bind(&preferred_genders)
        .bind(&preferred_professions)
        .bind(input.max_distance_km)
        .bind(input.only_verified)
        .bind(input.only_students)
        .execute(&state.db)
        .await?;

        // Fetch and return
        let row = sqlx::query_as::<_, PreferencesRow>(
            "SELECT min_age, max_age, preferred_genders, preferred_professions, max_distance, only_verified, only_students FROM user_preferences WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_one(&state.db)
        .await?;

        Ok(UserPreferences {
            min_age: row.min_age,
            max_age: row.max_age,
            preferred_genders: row.preferred_genders
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
            preferred_professions: row.preferred_professions
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
            max_distance_km: row.max_distance,
            only_verified: row.only_verified.unwrap_or(false),
            only_students: row.only_students.unwrap_or(false),
        })
    }

    /// Like a user
    async fn like_user(&self, ctx: &Context<'_>, target_user_id: i64) -> Result<Match> {
        let state = ctx.data::<AppState>()?;
        let user_id = get_user_id_from_context(ctx)?;

        if user_id == target_user_id {
            return Err(Error::new("Cannot like yourself"));
        }

        // Record the swipe
        sqlx::query(
            r#"
            INSERT INTO swipes (from_user_id, to_user_id, action, created_at)
            VALUES ($1, $2, 'like', NOW())
            ON CONFLICT (from_user_id, to_user_id) DO UPDATE SET action = 'like', created_at = NOW()
            "#,
        )
        .bind(user_id)
        .bind(target_user_id)
        .execute(&state.db)
        .await?;

        // Check for mutual like
        let mutual = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM swipes WHERE from_user_id = $1 AND to_user_id = $2 AND action = 'like'",
        )
        .bind(target_user_id)
        .bind(user_id)
        .fetch_one(&state.db)
        .await? > 0;

        if mutual {
            // Create or update match
            let match_id = sqlx::query_scalar::<_, uuid::Uuid>(
                r#"
                INSERT INTO matches (user1_id, user2_id, is_mutual_match, status, matched_at, created_at)
                VALUES (LEAST($1, $2), GREATEST($1, $2), TRUE, 'active', NOW(), NOW())
                ON CONFLICT (user1_id, user2_id) DO UPDATE SET is_mutual_match = TRUE, matched_at = NOW()
                RETURNING id
                "#,
            )
            .bind(user_id)
            .bind(target_user_id)
            .fetch_one(&state.db)
            .await?;

            let loader = ctx.data::<DataLoader<UserLoader>>()?;
            let partner = loader.load_one(target_user_id).await?;

            Ok(Match {
                id: match_id.to_string(),
                user1_id: user_id.min(target_user_id),
                user2_id: user_id.max(target_user_id),
                is_mutual: true,
                status: Some("active".to_string()),
                matched_at: Some(chrono::Utc::now().to_string()),
                partner,
                like_type: Some("swipe".to_string()),
                message: None,
            })
        } else {
            Ok(Match {
                id: "pending".to_string(),
                user1_id: user_id,
                user2_id: target_user_id,
                is_mutual: false,
                status: Some("pending".to_string()),
                matched_at: None,
                partner: None,
                like_type: Some("swipe".to_string()),
                message: None,
            })
        }
    }

    /// Pass on a user
    async fn pass_user(&self, ctx: &Context<'_>, target_user_id: i64) -> Result<bool> {
        let state = ctx.data::<AppState>()?;
        let user_id = get_user_id_from_context(ctx)?;

        sqlx::query(
            r#"
            INSERT INTO swipes (from_user_id, to_user_id, action, created_at)
            VALUES ($1, $2, 'pass', NOW())
            ON CONFLICT (from_user_id, to_user_id) DO UPDATE SET action = 'pass', created_at = NOW()
            "#,
        )
        .bind(user_id)
        .bind(target_user_id)
        .execute(&state.db)
        .await?;

        Ok(true)
    }

    /// Send a chat message
    async fn send_chat_message(
        &self,
        ctx: &Context<'_>,
        match_id: String,
        content: String,
    ) -> Result<Message> {
        let state = ctx.data::<AppState>()?;
        let user_id = get_user_id_from_context(ctx)?;
        // Verify user is part of this match and get receiver (match_id is VARCHAR)
        let match_row = sqlx::query_as::<_, MatchRow>(
            "SELECT id, user1_id, user2_id, is_mutual_match, status, created_at as matched_at FROM matches WHERE id = $1 AND (user1_id = $2 OR user2_id = $2)",
        )
        .bind(&match_id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| Error::new("Match not found"))?;

        let receiver_id = if match_row.user1_id == user_id {
            match_row.user2_id
        } else {
            match_row.user1_id
        };

        // Insert message
        let msg = sqlx::query_as::<_, MessageRow>(
            r#"
            INSERT INTO messages (match_id, sender_id, receiver_id, content, created_at, is_read)
            VALUES ($1, $2, $3, $4, NOW(), FALSE)
            RETURNING id, match_id, sender_id, receiver_id, content, created_at, is_read
            "#,
        )
        .bind(&match_id)
        .bind(user_id)
        .bind(receiver_id)
        .bind(&content)
        .fetch_one(&state.db)
        .await?;

        // Graph: user messaged receiver
        {
            let db = state.db.clone();
            let sid = user_id.to_string();
            let rid = receiver_id.to_string();
            tokio::spawn(async move {
                let _ = sqlx::query(
                    "INSERT INTO graph_edge_links_fwd (from_type, from_id, edge_type, to_type, to_id) VALUES ('user', $1, 'messaged', 'user', $2) ON CONFLICT DO NOTHING"
                ).bind(&sid).bind(&rid).execute(&db).await;
                let _ = sqlx::query(
                    "INSERT INTO graph_edge_links_rev (to_type, to_id, edge_type, from_type, from_id) VALUES ('user', $2, 'messaged', 'user', $1) ON CONFLICT DO NOTHING"
                ).bind(&sid).bind(&rid).execute(&db).await;
            });
        }

        // Auto-queue message for LLM labeling (toxicity, intent)
        auto_queue_for_labeling(state.db.clone(), state.config.llm_enabled, "message", msg.id as i64, 3);

        Ok(Message {
            id: msg.id,
            match_id: msg.match_id,
            sender_id: msg.sender_id,
            receiver_id: msg.receiver_id,
            content: msg.content,
            created_at: msg.created_at.map(|dt| dt.to_string()),
            is_read: msg.is_read.unwrap_or(false),
        })
    }

    /// Add a custom profession option (for when user selects "Other")
    /// Returns the created option ID as a simple string
    #[graphql(name = "add_profession_option")]
    async fn add_profession_option(
        &self,
        ctx: &Context<'_>,
        category: String,
        #[graphql(name = "name")] title: String,
    ) -> Result<String> {
        let _state = ctx.data::<AppState>()?;
        let _user_id = get_user_id_from_context(ctx)?;

        // Return just the ID as a scalar - no subfield selection needed
        let id = format!("custom_{}_{}", category, title.to_lowercase().replace(' ', "_"));
        Ok(id)
    }

    /// Upload voice intro - accepts base64 encoded audio or URL
    #[graphql(name = "upload_voice_intro")]
    async fn upload_voice_intro(
        &self,
        ctx: &Context<'_>,
        #[graphql(name = "voice_url")] voice_url: Option<String>,
        #[graphql(name = "duration_seconds")] duration: i32,
    ) -> Result<VoiceIntroResult> {
        let state = ctx.data::<AppState>()?;
        let user_id = get_user_id_from_context(ctx)?;

        // Validate duration (max 30 seconds for voice intro)
        if duration <= 0 || duration > 30 {
            return Err(Error::new("Voice intro must be between 1-30 seconds"));
        }

        let url = voice_url.unwrap_or_default();
        if url.is_empty() {
            return Err(Error::new("Voice URL is required"));
        }

        // Update user's voice intro
        sqlx::query(
            "UPDATE users SET voice_intro_url = $1, voice_intro_duration = $2, updated_at = NOW() WHERE id = $3"
        )
        .bind(&url)
        .bind(duration)
        .bind(user_id)
        .execute(&state.db)
        .await?;

        Ok(VoiceIntroResult {
            success: true,
            url,
            duration,
        })
    }

    /// Like a user with detailed result
    #[graphql(name = "likeUser")]
    async fn like_user_v2(&self, ctx: &Context<'_>, target_user_id: i64) -> Result<LikeResult> {
        let state = ctx.data::<AppState>()?;
        let user_id = get_user_id_from_context(ctx)?;

        if user_id == target_user_id {
            return Err(Error::new("Cannot like yourself"));
        }

        // Check if target exists
        let target_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM users WHERE id = $1 AND is_active = TRUE)",
        )
        .bind(target_user_id)
        .fetch_one(&state.db)
        .await?;

        if !target_exists {
            return Err(Error::new("User not found"));
        }

        // Delegate to shared swipe_service (same code path as REST)
        let outcome = crate::services::swipe_service::execute_like(
            &state.db, user_id as i32, target_user_id as i32, "discover"
        ).await?;

        Ok(LikeResult {
            success: true,
            is_mutual: outcome.is_mutual,
            match_id: if outcome.is_mutual { Some(outcome.match_id) } else { None },
            message: if outcome.is_mutual { "It's a match!".to_string() } else { "Like sent".to_string() },
        })
    }

    /// Pass on a user
    #[graphql(name = "passUser")]
    async fn pass_user_v2(&self, ctx: &Context<'_>, target_user_id: i64) -> Result<bool> {
        let state = ctx.data::<AppState>()?;
        let user_id = get_user_id_from_context(ctx)?;

        // Delegate to shared swipe_service (same code path as REST)
        crate::services::swipe_service::execute_pass(
            &state.db, user_id as i32, target_user_id as i32, "discover"
        ).await?;

        Ok(true)
    }

    /// Track voice intro playback
    #[graphql(name = "trackVoicePlay")]
    async fn track_voice_play(
        &self,
        ctx: &Context<'_>,
        target_user_id: i64,
        #[graphql(name = "play_duration_seconds")] play_duration: Option<i32>,
    ) -> Result<bool> {
        let state = ctx.data::<AppState>()?;
        let user_id = get_user_id_from_context(ctx)?;

        // Log the voice play event for ML training
        let _ = sqlx::query(
            r#"
            INSERT INTO interaction_events (user_id, target_user_id, event_type, surface, reward, created_at)
            VALUES ($1, $2, 'voice_play', 'discover', $3, NOW())
            "#
        )
        .bind(user_id)
        .bind(target_user_id)
        .bind(play_duration.map(|d| d as f64 / 30.0).unwrap_or(0.5)) // Normalize to 0-1 based on 30s max
        .execute(&state.db)
        .await;

        Ok(true)
    }
}

// ============================================================================
// Helper Types and Functions
// ============================================================================

#[derive(sqlx::FromRow)]
struct PreferencesRow {
    min_age: Option<i32>,
    max_age: Option<i32>,
    preferred_genders: Option<serde_json::Value>,
    preferred_professions: Option<serde_json::Value>,
    max_distance: Option<i32>,
    only_verified: Option<bool>,
    only_students: Option<bool>,
}

#[derive(sqlx::FromRow)]
struct DiscoverRow {
    id: i64,
    name: Option<String>,
    dob: Option<chrono::NaiveDate>,
    gender: Option<String>,
    bio: Option<String>,
    location_text: Option<String>,
    profile_photo_1: Option<String>,
    profile_photo_2: Option<String>,
    profile_photo_3: Option<String>,
    profile_photos: Option<serde_json::Value>,
    interests: Option<serde_json::Value>,
    languages: Option<serde_json::Value>,
    is_verified: Option<bool>,
    attractiveness_score: Option<rust_decimal::Decimal>,
    // Voice intro
    voice_intro_url: Option<String>,
    voice_intro_duration: Option<i32>,
    // Enhanced fields
    profession_title: Option<String>,
    profession_category: Option<String>,
    looking_for: Option<String>,
    height_cm: Option<i32>,
}

#[derive(sqlx::FromRow)]
struct MatchRow {
    id: String,
    user1_id: i64,
    user2_id: i64,
    is_mutual_match: Option<bool>,
    status: Option<String>,
    matched_at: Option<NaiveDateTime>,
    like_type: Option<String>,
    message: Option<String>,
}

#[derive(sqlx::FromRow)]
struct MatchCheckRow {
    id: String,
    user1_liked: Option<bool>,
    user2_liked: Option<bool>,
}

#[derive(sqlx::FromRow)]
struct LikedProfileRow {
    id: i64,
    name: Option<String>,
    dob: Option<chrono::NaiveDate>,
    profile_photo_1: Option<String>,
    profile_photo_url: Option<String>,
    profile_photos: Option<serde_json::Value>,
    university: Option<String>,
    profession_title: Option<String>,
    like_type: Option<String>,
    liked_at: Option<NaiveDateTime>,
    message: Option<String>,
}

fn liked_profile_row_to_type(row: LikedProfileRow) -> LikedProfile {
    let photo = row.profile_photo_1
        .or_else(|| {
            row.profile_photos
                .as_ref()
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .or(row.profile_photo_url);

    let age = row.dob.map(|dob| {
        let today = chrono::Utc::now().date_naive();
        today.years_since(dob).unwrap_or(0) as i32
    });

    LikedProfile {
        id: row.id as i64,
        name: row.name,
        age,
        photo,
        university: row.university,
        study: row.profession_title,
        like_type: row.like_type.unwrap_or_else(|| "like".to_string()),
        message: row.message,
        liked_at: row.liked_at.map(|dt| dt.to_string()),
    }
}

#[derive(sqlx::FromRow)]
struct MessageRow {
    id: i64,
    match_id: String,
    sender_id: i64,
    receiver_id: i64,
    content: String,
    created_at: Option<NaiveDateTime>,
    is_read: Option<bool>,
}

#[derive(sqlx::FromRow)]
struct StudentVerificationRow {
    university_name: Option<String>,
    discount_tier: Option<String>,
    expires_at: Option<NaiveDateTime>,
}

#[derive(sqlx::FromRow)]
struct UserCompatibilityRow {
    interests: Option<serde_json::Value>,
    languages: Option<serde_json::Value>,
    looking_for: Option<String>,
    gender: Option<String>,
}

#[derive(sqlx::FromRow)]
struct UniversitySearchRow {
    id: i64,
    name: String,
    domain: Option<String>,
    country: Option<String>,
    tier: Option<String>,
    student_count: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct UniversityInfoRow {
    id: i64,
    name: String,
    domain: Option<String>,
    country: Option<String>,
    tier: Option<String>,
}

/// Convert a DiscoverRow to a GraphQL DiscoverProfile
fn discover_row_to_profile(r: DiscoverRow) -> DiscoverProfile {
    let age = r.dob.map(|dob| {
        let today = chrono::Utc::now().date_naive();
        let mut age = today.year() - dob.year();
        if today.ordinal() < dob.ordinal() {
            age -= 1;
        }
        age
    });

    let photos: Vec<String> = r.profile_photos
        .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
        .unwrap_or_else(|| {
            [r.profile_photo_1, r.profile_photo_2, r.profile_photo_3]
                .into_iter()
                .flatten()
                .collect()
        });

    let interests: Vec<String> = r.interests
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    let languages: Vec<String> = r.languages
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    let has_voice = r.voice_intro_url.is_some();

    DiscoverProfile {
        id: i64::from(r.id),
        name: r.name,
        age,
        gender: r.gender,
        bio: r.bio,
        location: r.location_text,
        photos,
        interests,
        compatibility_score: None,
        distance_km: None,
        is_verified: r.is_verified.unwrap_or(false),
        voice_intro_url: r.voice_intro_url,
        voice_intro_duration: r.voice_intro_duration,
        has_voice_intro: has_voice,
        profession_title: r.profession_title,
        languages,
        looking_for: r.looking_for,
        height_cm: r.height_cm,
    }
}

fn get_user_id_from_context(ctx: &Context<'_>) -> Result<i64> {
    ctx.data_opt::<i64>()
        .copied()
        .ok_or_else(|| Error::new("Authentication required"))
}

/// Calculate compatibility score between two users (0-100)
fn calculate_compatibility(
    my_interests: &[String],
    my_languages: &[String],
    my_looking_for: &Option<String>,
    my_preferred_professions: &[String],
    their_interests: &[String],
    their_languages: &[String],
    their_looking_for: &Option<String>,
    their_profession_category: &Option<String>,
    has_voice_intro: bool,
    is_verified: bool,
) -> f64 {
    let mut score = 50.0; // Base score

    // Interest overlap (up to +25 points)
    if !my_interests.is_empty() && !their_interests.is_empty() {
        let my_set: std::collections::HashSet<_> = my_interests.iter().map(|s| s.to_lowercase()).collect();
        let their_set: std::collections::HashSet<_> = their_interests.iter().map(|s| s.to_lowercase()).collect();
        let overlap = my_set.intersection(&their_set).count();
        let max_possible = my_set.len().min(their_set.len()).max(1);
        let interest_score = (overlap as f64 / max_possible as f64) * 25.0;
        score += interest_score;
    }

    // Language match (up to +15 points)
    if !my_languages.is_empty() && !their_languages.is_empty() {
        let my_langs: std::collections::HashSet<_> = my_languages.iter().map(|s| s.to_lowercase()).collect();
        let their_langs: std::collections::HashSet<_> = their_languages.iter().map(|s| s.to_lowercase()).collect();
        let lang_overlap = my_langs.intersection(&their_langs).count();
        if lang_overlap > 0 {
            score += 15.0;
        }
    }

    // Looking for match (+10 points)
    if let (Some(mine), Some(theirs)) = (my_looking_for, their_looking_for) {
        if mine.to_lowercase() == theirs.to_lowercase() {
            score += 10.0;
        }
    }

    // Profession preference match (+10 points) - boost if candidate matches preferred profession
    if !my_preferred_professions.is_empty() {
        if let Some(their_prof) = their_profession_category {
            let pref_set: std::collections::HashSet<_> = my_preferred_professions.iter().map(|s| s.to_lowercase()).collect();
            if pref_set.contains(&their_prof.to_lowercase()) {
                score += 10.0;
            }
        }
    }

    // Voice intro bonus (+5 points) - shows effort/authenticity
    if has_voice_intro {
        score += 5.0;
    }

    // Verified bonus (+5 points) - trust signal
    if is_verified {
        score += 5.0;
    }

    // Add some randomness to avoid identical scores (±5%)
    let variance = (rand::random::<f64>() - 0.5) * 10.0;
    score += variance;

    // Clamp to 0-100 range
    score.max(0.0).min(100.0).round()
}

// ============================================================================
// Schema Builder
// ============================================================================

pub type AppSchema = Schema<QueryRoot, MutationRoot, EmptySubscription>;

pub fn build_schema(state: AppState) -> AppSchema {
    let user_loader = DataLoader::new(
        UserLoader { pool: state.db.clone() },
        tokio::spawn,
    );

    Schema::build(QueryRoot, MutationRoot, EmptySubscription)
        .data(state)
        .data(user_loader)
        // ====================================================================
        // Production Security: Query Complexity & Depth Limits
        // ====================================================================
        // These limits prevent GraphQL DoS attacks:
        // - Deeply nested queries that exhaust server resources
        // - Queries with excessive field selections
        // - Recursive queries that create exponential load
        //
        // Complexity: Each field costs 1, nested objects multiply
        // Example: user { matches { partner { ... } } } = 1 + 1*10 + 10*10 = 111
        .limit_complexity(200)  // Max complexity score for a single query
        .limit_depth(7)         // Max nesting depth (user->matches->partner->... max 7 levels)
        // ====================================================================
        .finish()
}

// ============================================================================
// Photo processing helper (GraphQL update_profile)
// ============================================================================

/// Run one profile photo through the full pipeline (resize, NSFW, EXIF strip,
/// quality) and persist it via the storage backend (disk or object store).
/// Returns the public "/uploads/{key}" URL, Ok(None) on soft failures
/// (encode/store), or a GraphQL error when the photo is rejected by policy.
/// Shared by the Upload-file path and the base64 data-URI path.
pub(crate) async fn process_and_store_profile_photo(
    state: &AppState,
    user_id: i64,
    slot_idx: usize,
    content: Vec<u8>,
) -> Result<Option<String>, async_graphql::Error> {
    use crate::services::photo_pipeline::{run_pipeline, PhotoVerdict, PipelineTimeouts};

    if content.is_empty() {
        return Ok(None);
    }

    let photo_slot = format!("profile_photo_{}", slot_idx + 1);
    let pipeline_timeouts = PipelineTimeouts::default();

    let pipeline_result = match run_pipeline(
        content,
        user_id as i32,
        &photo_slot,
        state.vision.clone(),
        state.moderation.clone(),
        &state.db,
        &pipeline_timeouts,
    ).await {
        Ok(r) => r,
        Err(e) => return Err(async_graphql::Error::new(format!("Photo processing failed: {}", e))),
    };

    if pipeline_result.verdict == PhotoVerdict::Rejected {
        let reason = pipeline_result.stages.iter()
            .find(|s| !s.passed)
            .and_then(|s| s.detail.as_deref())
            .unwrap_or("policy violation");
        return Err(async_graphql::Error::new(format!("Photo {} rejected: {}", slot_idx + 1, reason)));
    }

    let image = match pipeline_result.processed_image.as_ref() {
        Some(img) => img,
        None => return Ok(None),
    };

    // Encode to JPEG
    let mut jpeg_bytes = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_bytes, 85);
    if encoder.encode(
        image.as_bytes(),
        image.width(),
        image.height(),
        image.color().into(),
    ).is_err() {
        return Ok(None);
    }

    let unique_name = format!("{}_photo_{}_{}.jpg", user_id, slot_idx + 1, uuid::Uuid::new_v4());
    // Keep a copy for async anti-fraud screening; the original moves into storage.
    let screen_copy = jpeg_bytes.clone();
    if state.storage.put_key_vec(&unique_name, jpeg_bytes, "image/jpeg").await.is_err() {
        return Ok(None);
    }
    // Anti-fraud: async celebrity / stolen-photo screening (Rekognition)
    crate::handlers::spawn_celebrity_screen(state, user_id, unique_name.clone(), screen_copy);

    Ok(Some(format!("/uploads/{}", unique_name)))
}
