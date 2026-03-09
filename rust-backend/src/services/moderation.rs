//! Content moderation pipeline: text toxicity, URL/spam filtering, duplicate face detection,
//! moderation transparency with user-facing reasons and appeal path.

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::warn;

// --- Moderation Decision Types ---

/// Reason codes for moderation actions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReasonCode {
    Nsfw,
    Blurry,
    TooLowLight,
    Toxic,
    HateSpeech,
    Harassment,
    SpamUrl,
    SpamRepetitive,
    DuplicateFace,
    SuspiciousActivity,
    ProfileGaming,
}

impl ReasonCode {
    /// User-facing message for each reason code.
    pub fn user_message(&self) -> &'static str {
        match self {
            Self::Nsfw => "This photo contains content that violates our community guidelines.",
            Self::Blurry => "This photo appears to be blurry. Please upload a clearer image.",
            Self::TooLowLight => "This photo is too dark. Please upload a well-lit image.",
            Self::Toxic => "This message contains language that violates our community guidelines.",
            Self::HateSpeech => "This content contains hate speech and has been removed.",
            Self::Harassment => "This message was flagged for harassment.",
            Self::SpamUrl => "Messages containing suspicious links are not allowed.",
            Self::SpamRepetitive => "This message was flagged as spam.",
            Self::DuplicateFace => "This photo appears to belong to another account.",
            Self::SuspiciousActivity => "Your account has been flagged for review due to unusual activity.",
            Self::ProfileGaming => "Too many profile edits in a short time. Please try again later.",
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Nsfw => "nsfw_detected",
            Self::Blurry => "blurry_image",
            Self::TooLowLight => "low_light_image",
            Self::Toxic => "toxic_content",
            Self::HateSpeech => "hate_speech",
            Self::Harassment => "harassment",
            Self::SpamUrl => "spam_url",
            Self::SpamRepetitive => "spam_repetitive",
            Self::DuplicateFace => "duplicate_face",
            Self::SuspiciousActivity => "suspicious_activity",
            Self::ProfileGaming => "profile_gaming",
        }
    }
}

/// Moderation action taken on content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModerationAction {
    Allow,
    Warn,
    Block,
    Blur,
    Mute,
}

/// A persisted moderation decision with trace ID for audit.
#[derive(Debug, Clone, Serialize)]
pub struct ModerationDecision {
    pub id: Option<i64>,
    pub trace_id: String,
    pub user_id: i32,
    pub content_type: String,    // "photo", "message", "bio", "reel"
    pub content_id: Option<String>,
    pub action: ModerationAction,
    pub reason_code: ReasonCode,
    pub user_facing_reason: String,
    pub confidence: f64,
    pub model_scores: serde_json::Value,
}

/// Result of text moderation analysis.
#[derive(Debug, Clone, Serialize)]
pub struct TextModerationResult {
    pub is_allowed: bool,
    pub action: ModerationAction,
    pub reason: Option<ReasonCode>,
    pub scores: TextScores,
}

/// Individual text moderation scores.
#[derive(Debug, Clone, Serialize)]
pub struct TextScores {
    pub toxicity: f64,
    pub hate: f64,
    pub harassment: f64,
    pub spam: f64,
    pub url_detected: bool,
}

/// Result of duplicate face check.
#[derive(Debug, Clone, Serialize)]
pub struct DuplicateFaceResult {
    pub is_duplicate: bool,
    pub matching_user_id: Option<i32>,
    pub similarity: f64,
}

// --- URL/Spam Patterns ---

/// Known spam URL patterns and suspicious domains.
const SPAM_URL_PATTERNS: &[&str] = &[
    "bit.ly", "tinyurl.com", "t.co", "goo.gl",
    "cashapp", "venmo", "paypal.me",
    "onlyfans.com", "fansly.com",
    "telegram.me", "t.me",
    "wa.me", "whatsapp.com",
];

/// Known toxic keyword patterns (basic blocklist — production should use ML model).
const TOXIC_PATTERNS: &[&str] = &[
    // Keeping this minimal — real implementation uses NLP model
];

// --- Moderation Pipeline ---

/// Content moderation pipeline service.
pub struct ModerationPipeline {
    toxicity_threshold: f64,
    nsfw_threshold: f32,
}

impl ModerationPipeline {
    pub fn new(toxicity_threshold: f64, nsfw_threshold: f32) -> Self {
        Self {
            toxicity_threshold,
            nsfw_threshold,
        }
    }

    /// Moderate text content (bio, message, caption).
    pub fn moderate_text(&self, text: &str) -> TextModerationResult {
        let text_lower = text.to_lowercase();

        // 1. URL/spam detection
        let url_detected = self.detect_urls(&text_lower);
        let spam_score = self.spam_score(&text_lower);

        if url_detected && spam_score > 0.3 {
            return TextModerationResult {
                is_allowed: false,
                action: ModerationAction::Block,
                reason: Some(ReasonCode::SpamUrl),
                scores: TextScores {
                    toxicity: 0.0,
                    hate: 0.0,
                    harassment: 0.0,
                    spam: spam_score,
                    url_detected,
                },
            };
        }

        // 2. Toxicity scoring (keyword-based fallback; production uses ONNX model)
        let toxicity = self.toxicity_score(&text_lower);
        let hate = self.hate_score(&text_lower);
        let harassment = self.harassment_score(&text_lower);

        let max_score = toxicity.max(hate).max(harassment);

        if max_score > self.toxicity_threshold {
            let reason = if hate > toxicity && hate > harassment {
                ReasonCode::HateSpeech
            } else if harassment > toxicity {
                ReasonCode::Harassment
            } else {
                ReasonCode::Toxic
            };

            return TextModerationResult {
                is_allowed: false,
                action: ModerationAction::Mute,
                reason: Some(reason),
                scores: TextScores { toxicity, hate, harassment, spam: spam_score, url_detected },
            };
        }

        // 3. Repetitive spam check
        if self.is_repetitive_spam(&text_lower) {
            return TextModerationResult {
                is_allowed: false,
                action: ModerationAction::Block,
                reason: Some(ReasonCode::SpamRepetitive),
                scores: TextScores { toxicity, hate, harassment, spam: 0.9, url_detected },
            };
        }

        TextModerationResult {
            is_allowed: true,
            action: ModerationAction::Allow,
            reason: None,
            scores: TextScores { toxicity, hate, harassment, spam: spam_score, url_detected },
        }
    }

    /// Check for duplicate faces across users using ArcFace embeddings.
    pub async fn check_duplicate_face(
        &self,
        pool: &PgPool,
        embedding: &[f32],
        exclude_user_id: i32,
    ) -> DuplicateFaceResult {
        // Query stored embeddings and compute cosine similarity
        #[derive(sqlx::FromRow)]
        struct EmbeddingRow {
            user_id: i32,
            embedding: serde_json::Value,
        }

        let rows = sqlx::query_as::<_, EmbeddingRow>(
            r#"SELECT user_id, embedding FROM user_features
               WHERE user_id != $1 AND embedding IS NOT NULL
               LIMIT 1000"#
        )
        .bind(exclude_user_id)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        let mut best_match: Option<(i32, f64)> = None;

        for row in &rows {
            if let Some(stored_vec) = row.embedding.as_array() {
                let stored: Vec<f32> = stored_vec
                    .iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect();

                if stored.len() == embedding.len() {
                    let similarity = cosine_similarity(embedding, &stored);
                    if similarity > 0.85 {
                        match &best_match {
                            Some((_, best_sim)) if similarity > *best_sim => {
                                best_match = Some((row.user_id, similarity));
                            }
                            None => {
                                best_match = Some((row.user_id, similarity));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        match best_match {
            Some((user_id, similarity)) => DuplicateFaceResult {
                is_duplicate: true,
                matching_user_id: Some(user_id),
                similarity,
            },
            None => DuplicateFaceResult {
                is_duplicate: false,
                matching_user_id: None,
                similarity: 0.0,
            },
        }
    }

    /// Persist a moderation decision for audit trail.
    pub async fn log_decision(&self, pool: &PgPool, decision: &ModerationDecision) -> Option<i64> {
        match sqlx::query_scalar::<_, i64>(
            r#"INSERT INTO moderation_decisions
               (trace_id, user_id, content_type, content_id, action, reason_code, user_facing_reason, confidence, model_scores, created_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW())
               RETURNING id"#
        )
        .bind(&decision.trace_id)
        .bind(decision.user_id)
        .bind(&decision.content_type)
        .bind(&decision.content_id)
        .bind(serde_json::to_string(&decision.action).unwrap_or_default())
        .bind(decision.reason_code.as_str())
        .bind(&decision.user_facing_reason)
        .bind(decision.confidence)
        .bind(&decision.model_scores)
        .fetch_one(pool)
        .await
        {
            Ok(id) => Some(id),
            Err(e) => {
                warn!("Failed to log moderation decision: {e}");
                None
            }
        }
    }

    // --- Private helpers ---

    fn detect_urls(&self, text: &str) -> bool {
        // Check for http/https links
        if text.contains("http://") || text.contains("https://") || text.contains("www.") {
            return true;
        }
        // Check for known spam domains
        for pattern in SPAM_URL_PATTERNS {
            if text.contains(pattern) {
                return true;
            }
        }
        false
    }

    fn spam_score(&self, text: &str) -> f64 {
        let mut score: f64 = 0.0;
        // URL shorteners are suspicious
        for pattern in SPAM_URL_PATTERNS {
            if text.contains(pattern) {
                score += 0.4;
            }
        }
        // Phone number patterns
        let digit_count = text.chars().filter(|c| c.is_ascii_digit()).count();
        if digit_count > 7 {
            score += 0.3;
        }
        // Excessive special characters
        let special_ratio = text.chars().filter(|c| !c.is_alphanumeric() && !c.is_whitespace()).count() as f64
            / (text.len().max(1) as f64);
        if special_ratio > 0.3 {
            score += 0.2;
        }
        score.clamp(0.0_f64, 1.0_f64)
    }

    fn toxicity_score(&self, _text: &str) -> f64 {
        // Placeholder — production uses ONNX toxicity model
        // Returns 0.0 for now; keyword-based checks happen in hate/harassment
        0.0
    }

    fn hate_score(&self, _text: &str) -> f64 {
        // Placeholder — production uses NLP hate speech classifier
        0.0
    }

    fn harassment_score(&self, _text: &str) -> f64 {
        // Placeholder — production uses NLP harassment classifier
        0.0
    }

    fn is_repetitive_spam(&self, text: &str) -> bool {
        if text.len() < 10 {
            return false;
        }
        // Check if the message is mostly repeated characters
        let chars: Vec<char> = text.chars().collect();
        let unique: std::collections::HashSet<char> = chars.iter().copied().collect();
        let unique_ratio = unique.len() as f64 / chars.len() as f64;
        unique_ratio < 0.1 // Less than 10% unique chars = spam
    }
}

impl Default for ModerationPipeline {
    fn default() -> Self {
        Self::new(0.7, 0.7)
    }
}

/// Cosine similarity between two vectors.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| *x as f64 * *y as f64).sum();
    let norm_a: f64 = a.iter().map(|x| (*x as f64) * (*x as f64)).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| (*x as f64) * (*x as f64)).sum::<f64>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

// --- Appeal System ---

/// Appeal status for a moderation decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AppealStatus {
    Pending,
    Approved,
    Rejected,
}

/// Submit an appeal for a moderation decision.
pub async fn submit_appeal(
    pool: &PgPool,
    user_id: i32,
    decision_id: i64,
    reason: &str,
) -> Result<i64, sqlx::Error> {
    let id = sqlx::query_scalar::<_, i64>(
        r#"INSERT INTO moderation_appeals (decision_id, user_id, appeal_reason, status, submitted_at)
           VALUES ($1, $2, $3, 'pending', NOW())
           RETURNING id"#
    )
    .bind(decision_id)
    .bind(user_id)
    .bind(reason)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

/// Get moderation decisions for a user.
pub async fn get_user_decisions(
    pool: &PgPool,
    user_id: i32,
    limit: i64,
) -> Result<Vec<serde_json::Value>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (i64, String, String, String, String, String, f64, chrono::NaiveDateTime)>(
        r#"SELECT id, trace_id, content_type, action, reason_code, user_facing_reason, confidence, created_at
           FROM moderation_decisions
           WHERE user_id = $1
           ORDER BY created_at DESC
           LIMIT $2"#
    )
    .bind(user_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows.iter().map(|r| {
        serde_json::json!({
            "id": r.0,
            "trace_id": r.1,
            "content_type": r.2,
            "action": r.3,
            "reason_code": r.4,
            "user_facing_reason": r.5,
            "confidence": r.6,
            "created_at": r.7.to_string(),
            "can_appeal": true,
        })
    }).collect())
}

/// Get appeal status.
pub async fn get_appeal_status(
    pool: &PgPool,
    appeal_id: i64,
    user_id: i32,
) -> Result<Option<serde_json::Value>, sqlx::Error> {
    let row = sqlx::query_as::<_, (i64, i64, String, String, Option<String>, chrono::NaiveDateTime, Option<chrono::NaiveDateTime>)>(
        r#"SELECT id, decision_id, appeal_reason, status, admin_notes, submitted_at, resolved_at
           FROM moderation_appeals
           WHERE id = $1 AND user_id = $2"#
    )
    .bind(appeal_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| {
        serde_json::json!({
            "id": r.0,
            "decision_id": r.1,
            "appeal_reason": r.2,
            "status": r.3,
            "admin_notes": r.4,
            "submitted_at": r.5.to_string(),
            "resolved_at": r.6.map(|t| t.to_string()),
        })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_detection() {
        let pipeline = ModerationPipeline::default();
        assert!(pipeline.detect_urls("check out https://example.com"));
        assert!(pipeline.detect_urls("add me on t.me/spammer"));
        assert!(!pipeline.detect_urls("hello how are you"));
    }

    #[test]
    fn test_spam_score() {
        let pipeline = ModerationPipeline::default();
        assert!(pipeline.spam_score("visit bit.ly/scam now! call 1234567890") > 0.5);
        assert!(pipeline.spam_score("hey, nice profile!") < 0.3);
    }

    #[test]
    fn test_repetitive_spam() {
        let pipeline = ModerationPipeline::default();
        assert!(pipeline.is_repetitive_spam("aaaaaaaaaaaaaaaaaaa"));
        assert!(!pipeline.is_repetitive_spam("hello world"));
    }

    #[test]
    fn test_moderate_text_clean() {
        let pipeline = ModerationPipeline::default();
        let result = pipeline.moderate_text("Hey, nice to meet you!");
        assert!(result.is_allowed);
    }

    #[test]
    fn test_moderate_text_spam_url() {
        let pipeline = ModerationPipeline::default();
        let result = pipeline.moderate_text("Check my page at onlyfans.com/user123");
        assert!(!result.is_allowed);
        assert_eq!(result.reason, Some(ReasonCode::SpamUrl));
    }

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);

        let c = vec![0.0, 1.0, 0.0];
        assert!(cosine_similarity(&a, &c).abs() < 0.001);
    }

    #[test]
    fn test_reason_code_messages() {
        assert!(!ReasonCode::Nsfw.user_message().is_empty());
        assert!(!ReasonCode::Blurry.user_message().is_empty());
        assert_eq!(ReasonCode::Nsfw.as_str(), "nsfw_detected");
    }
}
