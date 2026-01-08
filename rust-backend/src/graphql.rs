use async_graphql::{
    Context, EmptySubscription, Error, InputObject, Object, Result, Schema, SimpleObject, Upload,
    dataloader::{DataLoader, Loader},
};
use chrono::NaiveDateTime;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;

use crate::auth::{create_access_token, decode_access_token};
use crate::config::Config;
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
    pub user_id: i32,
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
}

#[derive(InputObject)]
pub struct PreferencesInput {
    pub min_age: Option<i32>,
    pub max_age: Option<i32>,
    pub preferred_genders: Option<Vec<String>>,
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
        // Convert i64 keys to i32 for database query
        let int_keys: Vec<i32> = keys.iter().map(|&k| k as i32).collect();
        let rows = sqlx::query_as::<_, UserRow>(
            r#"
            SELECT id, phone_number, email, name, dob, gender, bio, location_text,
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

        // Convert i32 ids back to i64 for return
        Ok(rows.into_iter().map(|r| (i64::from(r.id), r.into())).collect())
    }
}

#[derive(sqlx::FromRow)]
struct UserRow {
    id: i32,
    phone_number: Option<String>,
    email: Option<String>,
    name: Option<String>,
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
            id: i64::from(r.id),
            phone_number: r.phone_number,
            email: r.email,
            name: r.name,
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
        let state = ctx.data::<AppState>()?;
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
            SELECT min_age, max_age, preferred_genders, max_distance, only_verified, only_students
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
            max_distance_km: r.max_distance,
            only_verified: r.only_verified.unwrap_or(false),
            only_students: r.only_students.unwrap_or(false),
        }))
    }

    /// Discover profiles for matching
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

        // Get profiles excluding already interacted users
        let rows = sqlx::query_as::<_, DiscoverRow>(
            r#"
            SELECT u.id, u.name, u.dob, u.gender, u.bio, u.location_text,
                   u.profile_photo_1, u.profile_photo_2, u.profile_photo_3, u.profile_photos,
                   u.interests, u.is_verified, u.attractiveness_score
            FROM users u
            WHERE u.id != $1
              AND u.is_profile_complete = true
              AND u.is_active = true
              AND u.id NOT IN (
                  SELECT target_user_id FROM swipes WHERE user_id = $1
              )
            ORDER BY u.attractiveness_score DESC NULLS LAST, u.created_at DESC
            LIMIT $2
            "#,
        )
        .bind(user_id)
        .bind(limit)
        .fetch_all(&state.db)
        .await?;

        Ok(rows.into_iter().map(|r| {
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

            let interests = r.interests
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default();

            DiscoverProfile {
                id: i64::from(r.id),
                name: r.name,
                age,
                gender: r.gender,
                bio: r.bio,
                location: r.location_text,
                photos,
                interests,
                compatibility_score: r.attractiveness_score.map(|d| d.to_string().parse().unwrap_or(0.0)),
                distance_km: None, // Would calculate from location
                is_verified: r.is_verified.unwrap_or(false),
            }
        }).collect())
    }

    /// Get all matches for current user
    async fn matches(&self, ctx: &Context<'_>) -> Result<Vec<Match>> {
        let state = ctx.data::<AppState>()?;
        let user_id = get_user_id_from_context(ctx)?;
        let loader = ctx.data::<DataLoader<UserLoader>>()?;

        let user_id_i32 = user_id as i32;
        let rows = sqlx::query_as::<_, MatchRow>(
            r#"
            SELECT id, user1_id, user2_id, is_mutual_match, status, created_at as matched_at
            FROM matches
            WHERE (user1_id = $1 OR user2_id = $1) AND is_mutual_match = true
            ORDER BY created_at DESC
            "#,
        )
        .bind(user_id_i32)
        .fetch_all(&state.db)
        .await?;

        let mut matches = Vec::new();
        for row in rows {
            let partner_id_i32 = if row.user1_id == user_id_i32 { row.user2_id } else { row.user1_id };
            let partner_id = i64::from(partner_id_i32);
            let partner = loader.load_one(partner_id).await?;

            matches.push(Match {
                id: row.id.clone(),
                user1_id: i64::from(row.user1_id),
                user2_id: i64::from(row.user2_id),
                is_mutual: row.is_mutual_match.unwrap_or(false),
                status: row.status,
                matched_at: row.matched_at.map(|dt| dt.to_string()),
                partner,
            });
        }

        Ok(matches)
    }

    /// Get conversation messages for a match
    async fn conversation(
        &self,
        ctx: &Context<'_>,
        match_id: String,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> Result<Vec<Message>> {
        let state = ctx.data::<AppState>()?;
        let user_id = get_user_id_from_context(ctx)?;
        let user_id_i32 = user_id as i32;

        // Verify user is part of this match (match_id is VARCHAR, not UUID)
        let match_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM matches WHERE id = $1 AND (user1_id = $2 OR user2_id = $2)",
        )
        .bind(&match_id)
        .bind(user_id_i32)
        .fetch_one(&state.db)
        .await?;

        if match_count == 0 {
            return Err(Error::new("Not authorized to view this conversation"));
        }

        let rows = sqlx::query_as::<_, MessageRow>(
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
        .await?;

        Ok(rows.into_iter().map(|r| Message {
            id: i64::from(r.id),
            match_id: r.match_id,
            sender_id: i64::from(r.sender_id),
            receiver_id: i64::from(r.receiver_id),
            content: r.content,
            created_at: r.created_at.map(|dt| dt.to_string()),
            is_read: r.is_read.unwrap_or(false),
        }).collect())
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
    async fn send_otp(&self, ctx: &Context<'_>, phone_number: String) -> Result<OtpResponse> {
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
        let existing = sqlx::query_scalar::<_, i32>(
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
                let id = sqlx::query_scalar::<_, i32>(
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
        #[graphql(name = "profile_photo_1")] profile_photo_1: Option<String>,
        #[graphql(name = "profile_photo_2")] profile_photo_2: Option<String>,
        #[graphql(name = "profile_photo_3")] profile_photo_3: Option<String>,
        #[graphql(default)] photos: Vec<Upload>,
    ) -> Result<bool> {
        let state = ctx.data::<AppState>()?;
        let user_id = get_user_id_from_context(ctx)?;

        // Build dynamic update query
        let mut updates = Vec::new();
        let mut params: Vec<String> = Vec::new();

        if let Some(ref val) = name {
            updates.push(format!("name = ${}", params.len() + 2));
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

        // Filter out placeholder strings from profile_photo_N (they start with # for multipart uploads)
        let clean_photo_1 = profile_photo_1.filter(|p| !p.starts_with('#') && !p.is_empty());
        let clean_photo_2 = profile_photo_2.filter(|p| !p.starts_with('#') && !p.is_empty());
        let clean_photo_3 = profile_photo_3.filter(|p| !p.starts_with('#') && !p.is_empty());

        // Handle uploaded photo files - save them and collect URLs
        let mut uploaded_urls: Vec<String> = Vec::new();
        for upload in photos.into_iter() {
            let upload_value = upload.value(ctx)?;
            let filename = upload_value.filename.clone();

            // Generate unique filename
            let ext = filename.rsplit('.').next().unwrap_or("jpg");
            let unique_name = format!("{}_{}.{}", user_id, uuid::Uuid::new_v4(), ext);

            // Read file content using std::io::Read
            let mut reader = upload_value.into_read();
            let mut content = Vec::new();
            std::io::Read::read_to_end(&mut reader, &mut content).ok();

            // Save to uploads directory
            let upload_dir = std::path::Path::new("/app/uploads/photos");
            if !upload_dir.exists() {
                std::fs::create_dir_all(upload_dir).ok();
            }
            let file_path = upload_dir.join(&unique_name);
            std::fs::write(&file_path, &content).ok();

            let photo_url = format!("/uploads/photos/{}", unique_name);
            uploaded_urls.push(photo_url);
        }

        // Build photo array maintaining position order:
        // - If clean_photo_N is a valid URL string, use it
        // - If clean_photo_N is None but we have uploads, use next upload for that position
        // This allows users to replace individual photos while keeping others
        let mut upload_iter = uploaded_urls.into_iter();
        let final_photo_1 = clean_photo_1.clone().or_else(|| upload_iter.next());
        let final_photo_2 = clean_photo_2.clone().or_else(|| upload_iter.next());
        let final_photo_3 = clean_photo_3.clone().or_else(|| upload_iter.next());

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

        sqlx::query(
            r#"
            INSERT INTO user_preferences (user_id, min_age, max_age, preferred_genders, max_distance, only_verified, only_students, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
            ON CONFLICT (user_id) DO UPDATE SET
                min_age = COALESCE($2, user_preferences.min_age),
                max_age = COALESCE($3, user_preferences.max_age),
                preferred_genders = COALESCE($4, user_preferences.preferred_genders),
                max_distance = COALESCE($5, user_preferences.max_distance),
                only_verified = COALESCE($6, user_preferences.only_verified),
                only_students = COALESCE($7, user_preferences.only_students),
                updated_at = NOW()
            "#,
        )
        .bind(user_id)
        .bind(input.min_age)
        .bind(input.max_age)
        .bind(&preferred_genders)
        .bind(input.max_distance_km)
        .bind(input.only_verified)
        .bind(input.only_students)
        .execute(&state.db)
        .await?;

        // Fetch and return
        let row = sqlx::query_as::<_, PreferencesRow>(
            "SELECT min_age, max_age, preferred_genders, max_distance, only_verified, only_students FROM user_preferences WHERE user_id = $1",
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
            INSERT INTO swipes (user_id, target_user_id, action, created_at)
            VALUES ($1, $2, 'like', NOW())
            ON CONFLICT (user_id, target_user_id) DO UPDATE SET action = 'like', created_at = NOW()
            "#,
        )
        .bind(user_id)
        .bind(target_user_id)
        .execute(&state.db)
        .await?;

        // Check for mutual like
        let mutual = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM swipes WHERE user_id = $1 AND target_user_id = $2 AND action = 'like'",
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
            })
        }
    }

    /// Pass on a user
    async fn pass_user(&self, ctx: &Context<'_>, target_user_id: i64) -> Result<bool> {
        let state = ctx.data::<AppState>()?;
        let user_id = get_user_id_from_context(ctx)?;

        sqlx::query(
            r#"
            INSERT INTO swipes (user_id, target_user_id, action, created_at)
            VALUES ($1, $2, 'pass', NOW())
            ON CONFLICT (user_id, target_user_id) DO UPDATE SET action = 'pass', created_at = NOW()
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
        let user_id_i32 = user_id as i32;

        // Verify user is part of this match and get receiver (match_id is VARCHAR)
        let match_row = sqlx::query_as::<_, MatchRow>(
            "SELECT id, user1_id, user2_id, is_mutual_match, status, created_at as matched_at FROM matches WHERE id = $1 AND (user1_id = $2 OR user2_id = $2)",
        )
        .bind(&match_id)
        .bind(user_id_i32)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| Error::new("Match not found"))?;

        let receiver_id_i32 = if match_row.user1_id == user_id_i32 {
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
        .bind(user_id_i32)
        .bind(receiver_id_i32)
        .bind(&content)
        .fetch_one(&state.db)
        .await?;

        Ok(Message {
            id: i64::from(msg.id),
            match_id: msg.match_id,
            sender_id: i64::from(msg.sender_id),
            receiver_id: i64::from(msg.receiver_id),
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
}

// ============================================================================
// Helper Types and Functions
// ============================================================================

#[derive(sqlx::FromRow)]
struct PreferencesRow {
    min_age: Option<i32>,
    max_age: Option<i32>,
    preferred_genders: Option<serde_json::Value>,
    max_distance: Option<i32>,
    only_verified: Option<bool>,
    only_students: Option<bool>,
}

#[derive(sqlx::FromRow)]
struct DiscoverRow {
    id: i32,
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
    is_verified: Option<bool>,
    attractiveness_score: Option<rust_decimal::Decimal>,
}

#[derive(sqlx::FromRow)]
struct MatchRow {
    id: String,
    user1_id: i32,
    user2_id: i32,
    is_mutual_match: Option<bool>,
    status: Option<String>,
    matched_at: Option<NaiveDateTime>,
}

#[derive(sqlx::FromRow)]
struct MessageRow {
    id: i32,
    match_id: String,
    sender_id: i32,
    receiver_id: i32,
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

fn get_user_id_from_context(ctx: &Context<'_>) -> Result<i64> {
    ctx.data_opt::<i64>()
        .copied()
        .ok_or_else(|| Error::new("Authentication required"))
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
        .finish()
}
