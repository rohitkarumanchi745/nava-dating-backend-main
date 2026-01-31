//! Ads Monetization Service
//!
//! Supports multiple ad networks with server-side tracking:
//! - Google AdMob
//! - Meta Audience Network (Facebook)
//! - Unity Ads
//!
//! Features:
//! - Location-based ad targeting
//! - Native language ad content
//! - Regional ad network selection
//!
//! Ad types:
//! - Banner ads (low revenue, non-intrusive)
//! - Interstitial ads (medium revenue, between screens)
//! - Native ads (medium revenue, in-feed)
//! - Rewarded ads (high revenue, user-initiated)

use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::error::AppError;

// ============================================================================
// LOCATION & LANGUAGE TARGETING
// ============================================================================

/// Supported languages for ad content
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AdLanguage {
    English,
    Hindi,
    Telugu,
    Tamil,
    Kannada,
    Malayalam,
    Marathi,
    Bengali,
    Gujarati,
    Punjabi,
    Spanish,
    Portuguese,
    Arabic,
}

impl AdLanguage {
    pub fn as_str(&self) -> &'static str {
        match self {
            AdLanguage::English => "en",
            AdLanguage::Hindi => "hi",
            AdLanguage::Telugu => "te",
            AdLanguage::Tamil => "ta",
            AdLanguage::Kannada => "kn",
            AdLanguage::Malayalam => "ml",
            AdLanguage::Marathi => "mr",
            AdLanguage::Bengali => "bn",
            AdLanguage::Gujarati => "gu",
            AdLanguage::Punjabi => "pa",
            AdLanguage::Spanish => "es",
            AdLanguage::Portuguese => "pt",
            AdLanguage::Arabic => "ar",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "hi" | "hindi" => AdLanguage::Hindi,
            "te" | "telugu" => AdLanguage::Telugu,
            "ta" | "tamil" => AdLanguage::Tamil,
            "kn" | "kannada" => AdLanguage::Kannada,
            "ml" | "malayalam" => AdLanguage::Malayalam,
            "mr" | "marathi" => AdLanguage::Marathi,
            "bn" | "bengali" => AdLanguage::Bengali,
            "gu" | "gujarati" => AdLanguage::Gujarati,
            "pa" | "punjabi" => AdLanguage::Punjabi,
            "es" | "spanish" => AdLanguage::Spanish,
            "pt" | "portuguese" => AdLanguage::Portuguese,
            "ar" | "arabic" => AdLanguage::Arabic,
            _ => AdLanguage::English,
        }
    }

    /// Get language name in native script
    pub fn native_name(&self) -> &'static str {
        match self {
            AdLanguage::English => "English",
            AdLanguage::Hindi => "हिन्दी",
            AdLanguage::Telugu => "తెలుగు",
            AdLanguage::Tamil => "தமிழ்",
            AdLanguage::Kannada => "ಕನ್ನಡ",
            AdLanguage::Malayalam => "മലയാളം",
            AdLanguage::Marathi => "मराठी",
            AdLanguage::Bengali => "বাংলা",
            AdLanguage::Gujarati => "ગુજરાતી",
            AdLanguage::Punjabi => "ਪੰਜਾਬੀ",
            AdLanguage::Spanish => "Español",
            AdLanguage::Portuguese => "Português",
            AdLanguage::Arabic => "العربية",
        }
    }
}

/// Indian state to language mapping
pub fn get_language_for_indian_state(state_code: &str) -> AdLanguage {
    match state_code.to_uppercase().as_str() {
        // Telugu-speaking states
        "AP" | "TS" => AdLanguage::Telugu,  // Andhra Pradesh, Telangana
        // Tamil-speaking
        "TN" => AdLanguage::Tamil,           // Tamil Nadu
        // Kannada-speaking
        "KA" => AdLanguage::Kannada,         // Karnataka
        // Malayalam-speaking
        "KL" => AdLanguage::Malayalam,       // Kerala
        // Marathi-speaking
        "MH" | "GA" => AdLanguage::Marathi,  // Maharashtra, Goa
        // Bengali-speaking
        "WB" => AdLanguage::Bengali,         // West Bengal
        // Gujarati-speaking
        "GJ" => AdLanguage::Gujarati,        // Gujarat
        // Punjabi-speaking
        "PB" => AdLanguage::Punjabi,         // Punjab
        // Hindi-speaking states (Hindi Belt)
        "UP" | "MP" | "BR" | "RJ" | "HR" | "JH" | "CG" | "UK" | "HP" | "DL" => AdLanguage::Hindi,
        // Default to Hindi for other Indian states
        _ => AdLanguage::Hindi,
    }
}

/// Get language for country code
pub fn get_language_for_country(country_code: &str) -> AdLanguage {
    match country_code.to_uppercase().as_str() {
        "IN" => AdLanguage::Hindi,  // Default for India (will be overridden by state)
        "US" | "GB" | "AU" | "CA" | "NZ" | "IE" => AdLanguage::English,
        "ES" | "MX" | "AR" | "CO" | "PE" | "CL" => AdLanguage::Spanish,
        "BR" | "PT" => AdLanguage::Portuguese,
        "AE" | "SA" | "EG" | "QA" | "KW" | "OM" | "BH" => AdLanguage::Arabic,
        _ => AdLanguage::English,
    }
}

/// Regional ad configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionalAdConfig {
    pub country_code: String,
    pub state_code: Option<String>,
    pub city: Option<String>,
    pub language: AdLanguage,
    pub preferred_networks: Vec<AdNetwork>,
    pub ecpm_multiplier: f64,  // Higher in metros
}

/// Ad network providers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AdNetwork {
    AdMob,
    Facebook,
    Unity,
}

impl AdNetwork {
    pub fn as_str(&self) -> &'static str {
        match self {
            AdNetwork::AdMob => "admob",
            AdNetwork::Facebook => "facebook",
            AdNetwork::Unity => "unity",
        }
    }
}

/// Ad types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AdType {
    Banner,
    Interstitial,
    Native,
    Rewarded,
}

impl AdType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AdType::Banner => "banner",
            AdType::Interstitial => "interstitial",
            AdType::Native => "native",
            AdType::Rewarded => "rewarded",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "banner" => AdType::Banner,
            "interstitial" => AdType::Interstitial,
            "native" => AdType::Native,
            "rewarded" => AdType::Rewarded,
            _ => AdType::Interstitial,
        }
    }

    /// Average eCPM in USD (varies by region)
    pub fn avg_ecpm_usd(&self) -> f64 {
        match self {
            AdType::Banner => 0.50,        // $0.50 per 1000 impressions
            AdType::Interstitial => 4.00,  // $4.00 per 1000 impressions
            AdType::Native => 3.00,        // $3.00 per 1000 impressions
            AdType::Rewarded => 15.00,     // $15.00 per 1000 completions
        }
    }
}

/// Reward types for rewarded ads
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RewardType {
    Boost,
    SuperLike,
    PremiumHours,
    ExtraLikes,
    ProfileView,
}

impl RewardType {
    pub fn as_str(&self) -> &'static str {
        match self {
            RewardType::Boost => "boost",
            RewardType::SuperLike => "super_like",
            RewardType::PremiumHours => "premium_hours",
            RewardType::ExtraLikes => "extra_likes",
            RewardType::ProfileView => "profile_view",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "boost" => RewardType::Boost,
            "super_like" | "superlike" => RewardType::SuperLike,
            "premium_hours" => RewardType::PremiumHours,
            "extra_likes" => RewardType::ExtraLikes,
            "profile_view" => RewardType::ProfileView,
            _ => RewardType::ExtraLikes,
        }
    }

    /// Default reward amount
    pub fn default_amount(&self) -> i32 {
        match self {
            RewardType::Boost => 1,           // 1 free boost
            RewardType::SuperLike => 1,       // 1 super like
            RewardType::PremiumHours => 2,    // 2 hours premium
            RewardType::ExtraLikes => 5,      // 5 extra likes
            RewardType::ProfileView => 1,     // 1 profile view reveal
        }
    }
}

/// Ad targeting info for client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdTargeting {
    pub country_code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    pub language: String,
    /// Whether this is a metro city (higher eCPM)
    pub is_metro: bool,
    /// eCPM multiplier for this region
    pub ecpm_multiplier: f64,
}

impl AdTargeting {
    pub fn new(country_code: &str, state_code: Option<&str>, city: Option<&str>) -> Self {
        let language = if country_code == "IN" {
            state_code
                .map(|s| get_language_for_indian_state(s))
                .unwrap_or(AdLanguage::Hindi)
        } else {
            get_language_for_country(country_code)
        };

        let is_metro = city.map(|c| is_indian_metro(c)).unwrap_or(false);
        let ecpm_multiplier = calculate_ecpm_multiplier(country_code, is_metro);

        Self {
            country_code: country_code.to_string(),
            state_code: state_code.map(|s| s.to_string()),
            city: city.map(|c| c.to_string()),
            language: language.as_str().to_string(),
            is_metro,
            ecpm_multiplier,
        }
    }
}

/// Check if city is an Indian metro (higher ad rates)
pub fn is_indian_metro(city: &str) -> bool {
    let city_lower = city.to_lowercase();
    matches!(city_lower.as_str(),
        "mumbai" | "delhi" | "bangalore" | "bengaluru" | "hyderabad" |
        "chennai" | "kolkata" | "pune" | "ahmedabad" | "jaipur" |
        "lucknow" | "kanpur" | "nagpur" | "indore" | "thane" |
        "bhopal" | "visakhapatnam" | "pimpri" | "patna" | "vadodara" |
        "ghaziabad" | "ludhiana" | "agra" | "nashik" | "faridabad" |
        "meerut" | "rajkot" | "varanasi" | "srinagar" | "aurangabad" |
        "dhanbad" | "amritsar" | "navi mumbai" | "allahabad" | "ranchi" |
        "coimbatore" | "jabalpur" | "gwalior" | "vijayawada" | "madurai" |
        "noida" | "gurgaon" | "gurugram" | "surat" | "kochi" | "cochin"
    )
}

/// Calculate eCPM multiplier based on region
pub fn calculate_ecpm_multiplier(country_code: &str, is_metro: bool) -> f64 {
    let base = match country_code.to_uppercase().as_str() {
        "US" => 3.0,     // Highest eCPM
        "GB" | "UK" => 2.5,
        "AU" | "CA" => 2.0,
        "AE" | "SA" => 1.8,  // Gulf countries - high spending
        "SG" => 1.7,
        "DE" | "FR" => 1.5,
        "IN" => 0.8,     // India base (lower but high volume)
        _ => 1.0,
    };

    // Metros in India have 1.5x the eCPM
    if country_code == "IN" && is_metro {
        base * 1.5
    } else {
        base
    }
}

/// Ad placement configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdPlacement {
    pub placement_id: String,
    pub name: String,
    pub ad_type: AdType,
    pub location: String,

    // Network unit IDs (default)
    pub admob_unit_id: Option<String>,
    pub facebook_placement_id: Option<String>,
    pub unity_placement_id: Option<String>,

    // Targeting
    pub show_to_free_users: bool,
    pub show_to_premium_users: bool,
    pub frequency_cap_per_hour: i32,

    pub is_active: bool,
}

/// Regional ad unit configuration for language-specific ads
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionalAdUnit {
    pub language: AdLanguage,
    pub admob_unit_id: Option<String>,
    pub facebook_placement_id: Option<String>,
    pub unity_placement_id: Option<String>,
}

/// Ad request from client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdRequest {
    pub user_id: i32,
    pub placement_id: String,
    pub platform: String,      // 'ios', 'android'
    pub country_code: String,
    /// Indian state code (e.g., "TS", "AP", "TN") for regional targeting
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_code: Option<String>,
    /// City for metro targeting (higher eCPM in metros)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    /// User's preferred language (auto-detected from state if not provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

/// Ad response to client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdResponse {
    pub should_show: bool,
    pub placement_id: String,
    pub ad_type: String,

    // Network-specific data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admob_unit_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facebook_placement_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unity_placement_id: Option<String>,

    // For rewarded ads
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reward_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reward_amount: Option<i32>,

    // Language targeting for native language ads
    /// Detected language code (e.g., "te", "hi", "ta")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_code: Option<String>,
    /// Native language name (e.g., "తెలుగు", "हिन्दी")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_name: Option<String>,
    /// Regional ad targeting info for ad networks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub targeting: Option<AdTargeting>,
}

/// Ad impression tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdImpression {
    pub user_id: i32,
    pub placement_id: String,
    pub ad_network: AdNetwork,
    pub ad_type: AdType,
    pub revenue_usd_micro: i64,  // Revenue in micro USD
    pub ecpm_usd_micro: i64,
    pub clicked: bool,
    pub completed: bool,         // For rewarded ads
    pub platform: String,
    pub country_code: String,
    /// State code for regional analytics
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_code: Option<String>,
    /// City for metro analytics
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    /// Language the ad was served in
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_code: Option<String>,
}

/// Rewarded ad completion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardedAdCompletion {
    pub user_id: i32,
    pub placement_id: String,
    pub reward_type: RewardType,
    pub reward_amount: i32,
}

/// Ads service
pub struct AdsService {
    pool: PgPool,
}

impl AdsService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Check if user should see ads
    pub async fn should_show_ads(&self, user_id: i32) -> Result<bool, AppError> {
        // Check if user is premium
        let is_premium: Option<(bool,)> = sqlx::query_as(
            "SELECT is_premium FROM users WHERE id = $1"
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        match is_premium {
            Some((true,)) => Ok(false),  // Premium users don't see ads
            _ => Ok(true),               // Free users see ads
        }
    }

    /// Get ad placement configuration
    pub async fn get_placement(&self, placement_id: &str) -> Result<Option<AdPlacement>, AppError> {
        let placement = sqlx::query_as::<_, (
            String, String, String, String,
            Option<String>, Option<String>, Option<String>,
            bool, bool, i32, bool
        )>(
            r#"
            SELECT placement_id, name, placement_type, location,
                   admob_unit_id, facebook_placement_id, unity_placement_id,
                   show_to_free_users, show_to_premium_users, frequency_cap_per_hour, is_active
            FROM ad_placements
            WHERE placement_id = $1
            "#
        )
        .bind(placement_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(placement.map(|p| AdPlacement {
            placement_id: p.0,
            name: p.1,
            ad_type: match p.2.as_str() {
                "banner" => AdType::Banner,
                "interstitial" => AdType::Interstitial,
                "native" => AdType::Native,
                "rewarded" => AdType::Rewarded,
                _ => AdType::Banner,
            },
            location: p.3,
            admob_unit_id: p.4,
            facebook_placement_id: p.5,
            unity_placement_id: p.6,
            show_to_free_users: p.7,
            show_to_premium_users: p.8,
            frequency_cap_per_hour: p.9,
            is_active: p.10,
        }))
    }

    /// Check frequency cap for a user/placement
    pub async fn check_frequency_cap(&self, user_id: i32, placement_id: &str, cap: i32) -> Result<bool, AppError> {
        let count: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*) FROM ad_impressions
            WHERE user_id = $1 AND placement_id = $2
              AND created_at > NOW() - INTERVAL '1 hour'
            "#
        )
        .bind(user_id)
        .bind(placement_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(count.0 < cap as i64)
    }

    /// Request an ad for a user with location-based targeting
    pub async fn request_ad(&self, request: AdRequest) -> Result<AdResponse, AppError> {
        // Build targeting info from request
        let targeting = AdTargeting::new(
            &request.country_code,
            request.state_code.as_deref(),
            request.city.as_deref(),
        );

        // Determine user's language
        let language = request.language
            .as_ref()
            .map(|l| AdLanguage::from_str(l))
            .unwrap_or_else(|| {
                if request.country_code == "IN" {
                    request.state_code
                        .as_ref()
                        .map(|s| get_language_for_indian_state(s))
                        .unwrap_or(AdLanguage::Hindi)
                } else {
                    get_language_for_country(&request.country_code)
                }
            });

        // Check if user should see ads
        let should_show = self.should_show_ads(request.user_id).await?;
        if !should_show {
            return Ok(AdResponse {
                should_show: false,
                placement_id: request.placement_id,
                ad_type: "none".to_string(),
                admob_unit_id: None,
                facebook_placement_id: None,
                unity_placement_id: None,
                reward_type: None,
                reward_amount: None,
                language_code: Some(language.as_str().to_string()),
                language_name: Some(language.native_name().to_string()),
                targeting: Some(targeting),
            });
        }

        // Get placement config
        let placement = self.get_placement(&request.placement_id).await?
            .ok_or_else(|| AppError::not_found("Ad placement not found"))?;

        if !placement.is_active {
            return Ok(AdResponse {
                should_show: false,
                placement_id: request.placement_id,
                ad_type: "none".to_string(),
                admob_unit_id: None,
                facebook_placement_id: None,
                unity_placement_id: None,
                reward_type: None,
                reward_amount: None,
                language_code: Some(language.as_str().to_string()),
                language_name: Some(language.native_name().to_string()),
                targeting: Some(targeting),
            });
        }

        // Check frequency cap
        let within_cap = self.check_frequency_cap(
            request.user_id,
            &request.placement_id,
            placement.frequency_cap_per_hour
        ).await?;

        if !within_cap {
            return Ok(AdResponse {
                should_show: false,
                placement_id: request.placement_id,
                ad_type: placement.ad_type.as_str().to_string(),
                admob_unit_id: None,
                facebook_placement_id: None,
                unity_placement_id: None,
                reward_type: None,
                reward_amount: None,
                language_code: Some(language.as_str().to_string()),
                language_name: Some(language.native_name().to_string()),
                targeting: Some(targeting),
            });
        }

        // Try to get regional ad unit for user's language
        let regional_unit = self.get_regional_ad_unit(&request.placement_id, language).await?;

        // Use regional ad unit if available, otherwise fall back to default
        let (admob_unit_id, facebook_placement_id, unity_placement_id) = match regional_unit {
            Some(unit) => (
                unit.admob_unit_id.or(placement.admob_unit_id),
                unit.facebook_placement_id.or(placement.facebook_placement_id),
                unit.unity_placement_id.or(placement.unity_placement_id),
            ),
            None => (
                placement.admob_unit_id,
                placement.facebook_placement_id,
                placement.unity_placement_id,
            ),
        };

        // Build response with reward info for rewarded ads
        let (reward_type, reward_amount) = if placement.ad_type == AdType::Rewarded {
            let rt = match placement.location.as_str() {
                "boost_screen" => RewardType::Boost,
                "superlike_screen" => RewardType::SuperLike,
                _ => RewardType::ExtraLikes,
            };
            (Some(rt.as_str().to_string()), Some(rt.default_amount()))
        } else {
            (None, None)
        };

        tracing::info!(
            user_id = request.user_id,
            language = %language.as_str(),
            country = %request.country_code,
            state = ?request.state_code,
            is_metro = targeting.is_metro,
            "Serving ad with regional targeting"
        );

        Ok(AdResponse {
            should_show: true,
            placement_id: request.placement_id,
            ad_type: placement.ad_type.as_str().to_string(),
            admob_unit_id,
            facebook_placement_id,
            unity_placement_id,
            reward_type,
            reward_amount,
            language_code: Some(language.as_str().to_string()),
            language_name: Some(language.native_name().to_string()),
            targeting: Some(targeting),
        })
    }

    /// Get regional ad unit for a specific language
    pub async fn get_regional_ad_unit(
        &self,
        placement_id: &str,
        language: AdLanguage,
    ) -> Result<Option<RegionalAdUnit>, AppError> {
        let unit = sqlx::query_as::<_, (String, Option<String>, Option<String>, Option<String>)>(
            r#"
            SELECT language_code, admob_unit_id, facebook_placement_id, unity_placement_id
            FROM ad_placement_regional_units
            WHERE placement_id = $1 AND language_code = $2 AND is_active = true
            "#
        )
        .bind(placement_id)
        .bind(language.as_str())
        .fetch_optional(&self.pool)
        .await?;

        Ok(unit.map(|(_, admob, fb, unity)| RegionalAdUnit {
            language,
            admob_unit_id: admob,
            facebook_placement_id: fb,
            unity_placement_id: unity,
        }))
    }

    /// Track ad impression
    pub async fn track_impression(&self, impression: AdImpression) -> Result<i64, AppError> {
        let id: (i64,) = sqlx::query_as(
            r#"
            INSERT INTO ad_impressions (
                user_id, placement_id, ad_network, ad_type,
                revenue_usd_micro, ecpm_usd_micro, clicked, completed,
                platform, country_code, state_code, city, language_code
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            RETURNING id
            "#
        )
        .bind(impression.user_id)
        .bind(&impression.placement_id)
        .bind(impression.ad_network.as_str())
        .bind(impression.ad_type.as_str())
        .bind(impression.revenue_usd_micro)
        .bind(impression.ecpm_usd_micro)
        .bind(impression.clicked)
        .bind(impression.completed)
        .bind(&impression.platform)
        .bind(&impression.country_code)
        .bind(&impression.state_code)
        .bind(&impression.city)
        .bind(&impression.language_code)
        .fetch_one(&self.pool)
        .await?;

        tracing::debug!(
            user_id = impression.user_id,
            placement = %impression.placement_id,
            language = ?impression.language_code,
            state = ?impression.state_code,
            "Tracked ad impression with regional data"
        );

        Ok(id.0)
    }

    /// Process rewarded ad completion and grant reward
    pub async fn process_rewarded_completion(&self, completion: RewardedAdCompletion) -> Result<(), AppError> {
        // Start transaction
        let mut tx = self.pool.begin().await?;

        // Insert completion record
        sqlx::query(
            r#"
            INSERT INTO rewarded_ad_completions (user_id, placement_id, reward_type, reward_value)
            VALUES ($1, $2, $3, $4)
            "#
        )
        .bind(completion.user_id)
        .bind(&completion.placement_id)
        .bind(completion.reward_type.as_str())
        .bind(completion.reward_amount)
        .execute(&mut *tx)
        .await?;

        // Grant reward based on type
        match completion.reward_type {
            RewardType::Boost | RewardType::SuperLike => {
                let consumable_type = completion.reward_type.as_str();
                sqlx::query(
                    r#"
                    INSERT INTO user_consumables (user_id, consumable_type, balance, total_purchased)
                    VALUES ($1, $2, $3, $3)
                    ON CONFLICT (user_id, consumable_type)
                    DO UPDATE SET balance = user_consumables.balance + $3,
                                  total_purchased = user_consumables.total_purchased + $3,
                                  updated_at = NOW()
                    "#
                )
                .bind(completion.user_id)
                .bind(consumable_type)
                .bind(completion.reward_amount)
                .execute(&mut *tx)
                .await?;
            }
            RewardType::PremiumHours => {
                // Grant temporary premium access
                // This would extend their premium expiry or grant temporary premium
                // For now, we'll just log it
                tracing::info!(
                    "Granting {} hours premium to user {} from rewarded ad",
                    completion.reward_amount,
                    completion.user_id
                );
            }
            RewardType::ExtraLikes => {
                // Add to daily likes quota (stored in Redis typically)
                tracing::info!(
                    "Granting {} extra likes to user {} from rewarded ad",
                    completion.reward_amount,
                    completion.user_id
                );
            }
            RewardType::ProfileView => {
                // Grant profile view reveal
                sqlx::query(
                    r#"
                    INSERT INTO user_consumables (user_id, consumable_type, balance, total_purchased)
                    VALUES ($1, 'profile_view', $2, $2)
                    ON CONFLICT (user_id, consumable_type)
                    DO UPDATE SET balance = user_consumables.balance + $2,
                                  total_purchased = user_consumables.total_purchased + $2,
                                  updated_at = NOW()
                    "#
                )
                .bind(completion.user_id)
                .bind(completion.reward_amount)
                .execute(&mut *tx)
                .await?;
            }
        }

        tx.commit().await?;
        Ok(())
    }

    /// Get ad revenue stats
    pub async fn get_revenue_stats(&self, days: i32) -> Result<AdRevenueStats, AppError> {
        let stats: (i64, i64, i64, i64) = sqlx::query_as(
            r#"
            SELECT
                COUNT(*) as impressions,
                SUM(CASE WHEN clicked THEN 1 ELSE 0 END) as clicks,
                SUM(CASE WHEN completed THEN 1 ELSE 0 END) as completions,
                COALESCE(SUM(revenue_usd_micro), 0) as revenue_micro
            FROM ad_impressions
            WHERE created_at > NOW() - INTERVAL '1 day' * $1
            "#
        )
        .bind(days)
        .fetch_one(&self.pool)
        .await?;

        Ok(AdRevenueStats {
            period_days: days,
            total_impressions: stats.0,
            total_clicks: stats.1,
            total_completions: stats.2,
            revenue_usd: stats.3 as f64 / 1_000_000.0,
            ctr: if stats.0 > 0 { stats.1 as f64 / stats.0 as f64 * 100.0 } else { 0.0 },
            ecpm_usd: if stats.0 > 0 { stats.3 as f64 / stats.0 as f64 * 1000.0 / 1_000_000.0 } else { 0.0 },
        })
    }
}

/// Ad revenue statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdRevenueStats {
    pub period_days: i32,
    pub total_impressions: i64,
    pub total_clicks: i64,
    pub total_completions: i64,
    pub revenue_usd: f64,
    pub ctr: f64,           // Click-through rate percentage
    pub ecpm_usd: f64,      // Effective CPM in USD
}

/// Estimate monthly ad revenue based on DAU
pub fn estimate_monthly_ad_revenue(daily_active_users: i64, ads_per_user_per_day: f64) -> AdRevenueEstimate {
    let monthly_impressions = daily_active_users as f64 * ads_per_user_per_day * 30.0;

    // Assuming mix: 60% banner, 25% interstitial, 10% native, 5% rewarded
    let banner_revenue = monthly_impressions * 0.60 * AdType::Banner.avg_ecpm_usd() / 1000.0;
    let interstitial_revenue = monthly_impressions * 0.25 * AdType::Interstitial.avg_ecpm_usd() / 1000.0;
    let native_revenue = monthly_impressions * 0.10 * AdType::Native.avg_ecpm_usd() / 1000.0;
    let rewarded_revenue = monthly_impressions * 0.05 * AdType::Rewarded.avg_ecpm_usd() / 1000.0;

    let total = banner_revenue + interstitial_revenue + native_revenue + rewarded_revenue;

    AdRevenueEstimate {
        daily_active_users,
        ads_per_user_per_day,
        monthly_impressions: monthly_impressions as i64,
        estimated_monthly_revenue_usd: total,
        breakdown: AdRevenueBreakdown {
            banner: banner_revenue,
            interstitial: interstitial_revenue,
            native: native_revenue,
            rewarded: rewarded_revenue,
        },
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdRevenueEstimate {
    pub daily_active_users: i64,
    pub ads_per_user_per_day: f64,
    pub monthly_impressions: i64,
    pub estimated_monthly_revenue_usd: f64,
    pub breakdown: AdRevenueBreakdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdRevenueBreakdown {
    pub banner: f64,
    pub interstitial: f64,
    pub native: f64,
    pub rewarded: f64,
}
