use sqlx::PgPool;

/// Number of features per user in the ML feature vector.
pub const USER_FEATURE_DIM: usize = 14;

/// 14-dimensional user feature vector for ML scoring.
/// Covers profile attributes, richness signals, swipe behavior, and engagement.
#[derive(Debug, Clone)]
pub struct UserFeatures {
    pub user_id: i32,
    // --- Profile attributes (8) ---
    pub age_norm: f64,             // normalized age 0-1 (18-60)
    pub attractiveness: f64,       // attractiveness_score from users table
    pub profile_completeness: f64, // ratio of filled fields
    pub verification_score: f64,   // is_verified + is_student_verified
    pub photo_count: f64,          // normalized photo count (0-1)
    pub height_norm: f64,          // normalized height (0-1)
    pub has_profession: f64,       // 1.0 if profession_category is set
    pub gender_enc: f64,           // male=0.0, female=1.0, non_binary=0.5
    // --- Richness signals (3) ---
    pub intent_enc: f64,           // relationship=1.0, casual=0.5, friendship=0.25, unset=0.0
    pub language_count: f64,       // normalized language count (0-1, capped at 5)
    pub interest_count: f64,       // normalized interest count (0-1, capped at 10)
    // --- Swipe & engagement (3) ---
    pub activity_score: f64,       // recent interaction count normalized
    pub like_rate: f64,            // fraction of swipes that were likes (0-1)
    pub match_rate: f64,           // fraction of likes that became mutual matches (0-1)
}

impl UserFeatures {
    /// Extract features from the database for a given user.
    pub async fn from_db(pool: &PgPool, user_id: i32) -> Result<Self, sqlx::Error> {
        #[derive(sqlx::FromRow)]
        struct Row {
            dob: Option<chrono::NaiveDate>,
            attractiveness_score: Option<f64>,
            is_verified: Option<bool>,
            is_student_verified: Option<bool>,
            height_cm: Option<i32>,
            name: Option<String>,
            bio: Option<String>,
            gender: Option<String>,
            profile_photo_url: Option<String>,
            profile_photo_1: Option<String>,
            profile_photo_2: Option<String>,
            profile_photo_3: Option<String>,
            looking_for: Option<String>,
            profession_category: Option<String>,
            interests: Option<serde_json::Value>,
            languages: Option<serde_json::Value>,
        }

        let row = sqlx::query_as::<_, Row>(
            r#"SELECT dob, attractiveness_score, is_verified, is_student_verified,
                      height_cm, name, bio, gender, profile_photo_url,
                      profile_photo_1, profile_photo_2, profile_photo_3, looking_for,
                      profession_category, interests, languages
               FROM users WHERE id = $1"#,
        )
        .bind(user_id)
        .fetch_one(pool)
        .await?;

        // Age normalized to 0-1 range (18-60)
        let age_norm = row
            .dob
            .map(|dob| {
                let age = (chrono::Utc::now().date_naive() - dob).num_days() as f64 / 365.25;
                ((age - 18.0) / 42.0).clamp(0.0, 1.0)
            })
            .unwrap_or(0.5);

        let attractiveness = row.attractiveness_score.unwrap_or(0.5);

        let has_profession = if row.profession_category.is_some() { 1.0 } else { 0.0 };

        // Gender encoding
        let gender_enc = match row.gender.as_deref() {
            Some("male") => 0.0,
            Some("female") => 1.0,
            Some("non_binary") => 0.5,
            _ => 0.5, // default neutral
        };

        // Intent encoding
        let intent_enc = match row.looking_for.as_deref() {
            Some("relationship") | Some("long_term") => 1.0,
            Some("casual") | Some("short_term") => 0.5,
            Some("friendship") => 0.25,
            _ => 0.0,
        };

        // Language count (JSONB array)
        let language_count = row
            .languages
            .as_ref()
            .and_then(|v| v.as_array())
            .map(|arr| (arr.len() as f64 / 5.0).min(1.0))
            .unwrap_or(0.0);

        // Interest count (JSONB array)
        let interest_count = row
            .interests
            .as_ref()
            .and_then(|v| v.as_array())
            .map(|arr| (arr.len() as f64 / 10.0).min(1.0))
            .unwrap_or(0.0);

        // Count filled profile fields
        let filled: f64 = [
            row.name.is_some(),
            row.bio.is_some(),
            row.profile_photo_url.is_some(),
            row.looking_for.is_some(),
            row.height_cm.is_some(),
            row.profession_category.is_some(),
            row.gender.is_some(),
            row.interests.is_some(),
            row.languages.is_some(),
        ]
        .iter()
        .filter(|&&v| v)
        .count() as f64;
        let profile_completeness = filled / 9.0;

        let verification_score = match (
            row.is_verified.unwrap_or(false),
            row.is_student_verified.unwrap_or(false),
        ) {
            (true, true) => 1.0,
            (true, false) | (false, true) => 0.5,
            _ => 0.0,
        };

        // Activity score from recent interactions (7 days)
        let interaction_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM interaction_events WHERE user_id = $1 AND created_at > NOW() - INTERVAL '7 days'",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);
        let activity_score = (interaction_count as f64 / 50.0).min(1.0);

        // Swipe stats: like_rate and match_rate from interaction_events + matches
        #[derive(sqlx::FromRow)]
        struct SwipeStats {
            total_swipes: Option<i64>,
            total_likes: Option<i64>,
        }

        let swipe_stats = sqlx::query_as::<_, SwipeStats>(
            r#"SELECT
                COUNT(*) AS total_swipes,
                COUNT(*) FILTER (WHERE event_type = 'like') AS total_likes
               FROM interaction_events
               WHERE user_id = $1 AND event_type IN ('like', 'pass')"#,
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap_or(SwipeStats {
            total_swipes: Some(0),
            total_likes: Some(0),
        });

        let total_swipes = swipe_stats.total_swipes.unwrap_or(0);
        let total_likes = swipe_stats.total_likes.unwrap_or(0);
        let like_rate = if total_swipes > 0 {
            (total_likes as f64 / total_swipes as f64).clamp(0.0, 1.0)
        } else {
            0.5 // neutral default
        };

        // Match rate: mutual matches / total likes
        let mutual_match_count: i64 = sqlx::query_scalar(
            r#"SELECT COUNT(*) FROM matches
               WHERE is_mutual_match = TRUE
                 AND (user1_id = $1 OR user2_id = $1)"#,
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        let match_rate = if total_likes > 0 {
            (mutual_match_count as f64 / total_likes as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // Photo count
        let photos = [
            row.profile_photo_url.is_some(),
            row.profile_photo_1.is_some(),
            row.profile_photo_2.is_some(),
            row.profile_photo_3.is_some(),
        ]
        .iter()
        .filter(|&&v| v)
        .count() as f64;
        let photo_count = photos / 4.0;

        let height_norm = row
            .height_cm
            .map(|h| ((h as f64 - 140.0) / 60.0).clamp(0.0, 1.0))
            .unwrap_or(0.5);

        Ok(Self {
            user_id,
            age_norm,
            attractiveness,
            profile_completeness,
            verification_score,
            photo_count,
            height_norm,
            has_profession,
            gender_enc,
            intent_enc,
            language_count,
            interest_count,
            activity_score,
            like_rate,
            match_rate,
        })
    }

    /// Convert to 14-dim feature vector.
    pub fn to_vec(&self) -> Vec<f64> {
        vec![
            self.age_norm,
            self.attractiveness,
            self.profile_completeness,
            self.verification_score,
            self.photo_count,
            self.height_norm,
            self.has_profession,
            self.gender_enc,
            self.intent_enc,
            self.language_count,
            self.interest_count,
            self.activity_score,
            self.like_rate,
            self.match_rate,
        ]
    }
}

/// Population-level feature defaults for graceful fallback when per-user
/// features are unavailable (user not found, DB timeout, etc.).
#[derive(Debug, Clone)]
pub struct FeatureDefaults {
    pub defaults: UserFeatures,
}

impl FeatureDefaults {
    /// Hardcoded neutral defaults (fallback of fallbacks).
    pub fn hardcoded() -> Self {
        Self {
            defaults: UserFeatures {
                user_id: 0,
                age_norm: 0.5,
                attractiveness: 0.5,
                profile_completeness: 0.3,
                verification_score: 0.0,
                photo_count: 0.25,
                height_norm: 0.5,
                has_profession: 0.0,
                gender_enc: 0.5,
                intent_enc: 0.0,
                language_count: 0.2,
                interest_count: 0.2,
                activity_score: 0.1,
                like_rate: 0.5,
                match_rate: 0.1,
            },
        }
    }

    /// Compute population-level means from the database.
    pub async fn from_population(pool: &PgPool) -> Self {
        #[derive(sqlx::FromRow)]
        struct PopRow {
            avg_age: Option<f64>,
            avg_attractiveness: Option<f64>,
            avg_verified: Option<f64>,
            avg_height: Option<f64>,
            avg_photo_count: Option<f64>,
            avg_completeness: Option<f64>,
        }

        let row = sqlx::query_as::<_, PopRow>(
            r#"SELECT
                AVG(EXTRACT(EPOCH FROM AGE(NOW(), dob)) / 365.25) AS avg_age,
                AVG(attractiveness_score) AS avg_attractiveness,
                AVG(CASE WHEN is_verified THEN 1.0 ELSE 0.0 END) AS avg_verified,
                AVG(height_cm) AS avg_height,
                AVG(
                    (CASE WHEN profile_photo_url IS NOT NULL THEN 1 ELSE 0 END +
                     CASE WHEN profile_photo_1 IS NOT NULL THEN 1 ELSE 0 END +
                     CASE WHEN profile_photo_2 IS NOT NULL THEN 1 ELSE 0 END +
                     CASE WHEN profile_photo_3 IS NOT NULL THEN 1 ELSE 0 END)::float / 4.0
                ) AS avg_photo_count,
                AVG(
                    (CASE WHEN name IS NOT NULL THEN 1 ELSE 0 END +
                     CASE WHEN bio IS NOT NULL THEN 1 ELSE 0 END +
                     CASE WHEN profile_photo_url IS NOT NULL THEN 1 ELSE 0 END +
                     CASE WHEN looking_for IS NOT NULL THEN 1 ELSE 0 END +
                     CASE WHEN height_cm IS NOT NULL THEN 1 ELSE 0 END +
                     CASE WHEN profession_category IS NOT NULL THEN 1 ELSE 0 END +
                     CASE WHEN gender IS NOT NULL THEN 1 ELSE 0 END +
                     CASE WHEN interests IS NOT NULL THEN 1 ELSE 0 END +
                     CASE WHEN languages IS NOT NULL THEN 1 ELSE 0 END)::float / 9.0
                ) AS avg_completeness
            FROM users WHERE dob IS NOT NULL"#,
        )
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();

        let hc = Self::hardcoded().defaults;
        match row {
            Some(r) => {
                let age_norm = r.avg_age
                    .map(|a| ((a - 18.0) / 42.0).clamp(0.0, 1.0))
                    .unwrap_or(hc.age_norm);
                let height_norm = r.avg_height
                    .map(|h| ((h - 140.0) / 60.0).clamp(0.0, 1.0))
                    .unwrap_or(hc.height_norm);
                Self {
                    defaults: UserFeatures {
                        user_id: 0,
                        age_norm,
                        attractiveness: r.avg_attractiveness.unwrap_or(hc.attractiveness),
                        profile_completeness: r.avg_completeness.unwrap_or(hc.profile_completeness),
                        verification_score: r.avg_verified.unwrap_or(hc.verification_score),
                        photo_count: r.avg_photo_count.unwrap_or(hc.photo_count),
                        height_norm,
                        has_profession: hc.has_profession,
                        gender_enc: hc.gender_enc,
                        intent_enc: hc.intent_enc,
                        language_count: hc.language_count,
                        interest_count: hc.interest_count,
                        activity_score: hc.activity_score,
                        like_rate: hc.like_rate,
                        match_rate: hc.match_rate,
                    },
                }
            }
            None => Self::hardcoded(),
        }
    }

    /// Create a UserFeatures from defaults for a given user_id.
    pub fn for_user(&self, user_id: i32) -> UserFeatures {
        let mut f = self.defaults.clone();
        f.user_id = user_id;
        f
    }
}

/// Combine user + candidate features into a 28-dim state vector for RL.
pub fn combine_features(user: &UserFeatures, candidate: &UserFeatures) -> Vec<f64> {
    let mut v = user.to_vec();
    v.extend(candidate.to_vec());
    v
}

/// Feature schema describing each dimension — sent to FL clients so they
/// know what the model's input features represent.
pub fn feature_schema() -> serde_json::Value {
    serde_json::json!({
        "user_feature_dim": USER_FEATURE_DIM,
        "combined_state_dim": USER_FEATURE_DIM * 2,
        "features": [
            {"index": 0,  "name": "age_norm",             "type": "profile",    "range": [0.0, 1.0]},
            {"index": 1,  "name": "attractiveness",       "type": "profile",    "range": [0.0, 1.0]},
            {"index": 2,  "name": "profile_completeness", "type": "profile",    "range": [0.0, 1.0]},
            {"index": 3,  "name": "verification_score",   "type": "profile",    "range": [0.0, 1.0]},
            {"index": 4,  "name": "photo_count",          "type": "profile",    "range": [0.0, 1.0]},
            {"index": 5,  "name": "height_norm",          "type": "profile",    "range": [0.0, 1.0]},
            {"index": 6,  "name": "has_profession",       "type": "profile",    "range": [0.0, 1.0]},
            {"index": 7,  "name": "gender_enc",           "type": "profile",    "range": [0.0, 1.0]},
            {"index": 8,  "name": "intent_enc",           "type": "profile",    "range": [0.0, 1.0]},
            {"index": 9,  "name": "language_count",       "type": "richness",   "range": [0.0, 1.0]},
            {"index": 10, "name": "interest_count",       "type": "richness",   "range": [0.0, 1.0]},
            {"index": 11, "name": "activity_score",       "type": "engagement", "range": [0.0, 1.0]},
            {"index": 12, "name": "like_rate",            "type": "swipe",      "range": [0.0, 1.0]},
            {"index": 13, "name": "match_rate",           "type": "swipe",      "range": [0.0, 1.0]},
        ]
    })
}

/// FL training data for on-device model training.
/// Contains the user's profile features + their swipe history as labeled
/// training pairs, so the FL client can train a local recommendation model.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FLTrainingData {
    /// The user's own 14-dim feature vector
    pub user_features: Vec<f64>,
    /// Labeled training pairs: (candidate_features, label)
    /// label = 1.0 for like, 0.0 for pass, 1.5 for mutual match
    pub training_pairs: Vec<FLTrainingPair>,
    /// Feature schema so the client knows dimensions
    pub feature_schema: serde_json::Value,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FLTrainingPair {
    /// 28-dim combined state: user_features ++ candidate_features
    pub state: Vec<f64>,
    /// Reward label: 1.0 = liked, 0.0 = passed, 1.5 = mutual match
    pub label: f64,
    /// Candidate user's 14-dim features
    pub candidate_features: Vec<f64>,
}

impl FLTrainingData {
    /// Build FL training data for a user from their swipe history.
    /// Fetches the user's recent interactions and the features of each candidate.
    pub async fn build(pool: &PgPool, user_id: i32, max_pairs: i64) -> Result<Self, sqlx::Error> {
        let user_features = UserFeatures::from_db(pool, user_id).await?;
        let user_vec = user_features.to_vec();

        // Fetch recent swipe interactions with their outcomes
        #[derive(sqlx::FromRow)]
        struct InteractionRow {
            target_user_id: i32,
            event_type: String,
        }

        let interactions = sqlx::query_as::<_, InteractionRow>(
            r#"SELECT target_user_id, event_type
               FROM interaction_events
               WHERE user_id = $1 AND event_type IN ('like', 'pass')
               ORDER BY created_at DESC
               LIMIT $2"#,
        )
        .bind(user_id)
        .bind(max_pairs)
        .fetch_all(pool)
        .await?;

        // Check which likes became mutual matches
        let mutual_match_ids: Vec<i32> = sqlx::query_scalar(
            r#"SELECT CASE WHEN user1_id = $1 THEN user2_id ELSE user1_id END
               FROM matches
               WHERE is_mutual_match = TRUE AND (user1_id = $1 OR user2_id = $1)"#,
        )
        .bind(user_id)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        let mut training_pairs = Vec::with_capacity(interactions.len());
        for interaction in &interactions {
            let candidate = match UserFeatures::from_db(pool, interaction.target_user_id).await {
                Ok(f) => f,
                Err(_) => continue,
            };
            let candidate_vec = candidate.to_vec();
            let state = combine_features(&user_features, &candidate);

            let label = if interaction.event_type == "like" {
                if mutual_match_ids.contains(&interaction.target_user_id) {
                    1.5 // mutual match — strongest positive signal
                } else {
                    1.0 // liked
                }
            } else {
                0.0 // passed
            };

            training_pairs.push(FLTrainingPair {
                state,
                label,
                candidate_features: candidate_vec,
            });
        }

        Ok(FLTrainingData {
            user_features: user_vec,
            training_pairs,
            feature_schema: feature_schema(),
        })
    }
}
