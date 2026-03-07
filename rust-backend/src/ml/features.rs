use sqlx::PgPool;

/// 7-dimensional user feature vector for ML scoring.
/// Extracted from nava's `users` + `user_features` tables.
#[derive(Debug, Clone)]
pub struct UserFeatures {
    pub user_id: i32,
    pub age_norm: f64,           // normalized age (0-1)
    pub attractiveness: f64,     // attractiveness_score from users table
    pub profile_completeness: f64, // ratio of filled fields
    pub verification_score: f64, // is_verified + is_student_verified
    pub activity_score: f64,     // based on recent interactions
    pub photo_count: f64,        // normalized photo count (0-1)
    pub height_norm: f64,        // normalized height (0-1)
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
            profile_photo_url: Option<String>,
            profile_photo_1: Option<String>,
            profile_photo_2: Option<String>,
            profile_photo_3: Option<String>,
            looking_for: Option<String>,
        }

        let row = sqlx::query_as::<_, Row>(
            r#"SELECT dob, attractiveness_score, is_verified, is_student_verified,
                      height_cm, name, bio, profile_photo_url,
                      profile_photo_1, profile_photo_2, profile_photo_3, looking_for
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

        // Count filled profile fields
        let filled: f64 = [
            row.name.is_some(),
            row.bio.is_some(),
            row.profile_photo_url.is_some(),
            row.looking_for.is_some(),
            row.height_cm.is_some(),
        ]
        .iter()
        .filter(|&&v| v)
        .count() as f64;
        let profile_completeness = filled / 5.0;

        let verification_score = match (
            row.is_verified.unwrap_or(false),
            row.is_student_verified.unwrap_or(false),
        ) {
            (true, true) => 1.0,
            (true, false) | (false, true) => 0.5,
            _ => 0.0,
        };

        // Activity score from recent interactions
        let interaction_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM interaction_events WHERE user_id = $1 AND created_at > NOW() - INTERVAL '7 days'",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);
        let activity_score = (interaction_count as f64 / 50.0).min(1.0);

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
            activity_score,
            photo_count,
            height_norm,
        })
    }

    /// Convert to 7-dim feature vector.
    pub fn to_vec(&self) -> Vec<f64> {
        vec![
            self.age_norm,
            self.attractiveness,
            self.profile_completeness,
            self.verification_score,
            self.activity_score,
            self.photo_count,
            self.height_norm,
        ]
    }
}

/// Combine user + candidate features into a 14-dim state vector for RL.
pub fn combine_features(user: &UserFeatures, candidate: &UserFeatures) -> Vec<f64> {
    let mut v = user.to_vec();
    v.extend(candidate.to_vec());
    v
}
