//! Affinity scoring: interest/language overlap, intent alignment, and lightweight
//! collaborative filtering via implicit co-occurrence signals.
//!
//! Produces a 0.0–1.0 affinity score that boosts candidates who share meaningful
//! attributes with the user. The collaborative filtering component uses a
//! co-like matrix ("users who liked X also liked Y") to surface non-obvious matches.

use std::collections::HashMap;
use sqlx::PgPool;

/// Affinity scorer combining content overlap and collaborative filtering.
#[derive(Debug, Clone)]
pub struct AffinityScorer {
    /// Weight for interest overlap (Jaccard similarity)
    pub interest_weight: f64,
    /// Weight for language overlap
    pub language_weight: f64,
    /// Weight for intent alignment (relationship goals match)
    pub intent_weight: f64,
    /// Weight for collaborative filtering signal
    pub cf_weight: f64,
}

impl Default for AffinityScorer {
    fn default() -> Self {
        Self {
            interest_weight: 0.35,
            language_weight: 0.15,
            intent_weight: 0.20,
            cf_weight: 0.30,
        }
    }
}

/// Affinity breakdown for observability.
#[derive(Debug, Clone)]
pub struct AffinityScore {
    /// Combined affinity score (0.0-1.0)
    pub total: f64,
    /// Interest overlap (Jaccard index)
    pub interest_overlap: f64,
    /// Language overlap (Jaccard index)
    pub language_overlap: f64,
    /// Intent alignment (0.0, 0.5, or 1.0)
    pub intent_alignment: f64,
    /// CF score from co-like matrix
    pub cf_score: f64,
}

impl AffinityScorer {
    /// Compute interest overlap using Jaccard similarity.
    /// Interests are stored as JSON arrays of strings in the users table.
    pub fn interest_overlap(user_interests: &[String], candidate_interests: &[String]) -> f64 {
        if user_interests.is_empty() || candidate_interests.is_empty() {
            return 0.0;
        }
        let user_set: std::collections::HashSet<&str> =
            user_interests.iter().map(|s| s.as_str()).collect();
        let cand_set: std::collections::HashSet<&str> =
            candidate_interests.iter().map(|s| s.as_str()).collect();
        let intersection = user_set.intersection(&cand_set).count() as f64;
        let union = user_set.union(&cand_set).count() as f64;
        if union == 0.0 { 0.0 } else { intersection / union }
    }

    /// Compute language overlap using Jaccard similarity.
    pub fn language_overlap(user_langs: &[String], candidate_langs: &[String]) -> f64 {
        if user_langs.is_empty() || candidate_langs.is_empty() {
            return 0.0;
        }
        let user_set: std::collections::HashSet<&str> =
            user_langs.iter().map(|s| s.as_str()).collect();
        let cand_set: std::collections::HashSet<&str> =
            candidate_langs.iter().map(|s| s.as_str()).collect();
        let intersection = user_set.intersection(&cand_set).count() as f64;
        let union = user_set.union(&cand_set).count() as f64;
        if union == 0.0 { 0.0 } else { intersection / union }
    }

    /// Compute intent alignment between two users' relationship goals.
    /// Perfect match = 1.0, adjacent = 0.5, mismatch = 0.0.
    pub fn intent_alignment(user_intent: Option<&str>, candidate_intent: Option<&str>) -> f64 {
        match (user_intent, candidate_intent) {
            (Some(a), Some(b)) if a == b => 1.0,
            (Some(a), Some(b)) => {
                let rank = |s: &str| -> i32 {
                    match s {
                        "relationship" | "long_term" => 3,
                        "casual" | "short_term" => 2,
                        "friendship" => 1,
                        _ => 0,
                    }
                };
                let diff = (rank(a) - rank(b)).abs();
                if diff <= 1 { 0.5 } else { 0.0 }
            }
            _ => 0.25, // Unknown intent — neutral
        }
    }

    /// Compute combined affinity score.
    pub fn score(
        &self,
        user_interests: &[String],
        candidate_interests: &[String],
        user_langs: &[String],
        candidate_langs: &[String],
        user_intent: Option<&str>,
        candidate_intent: Option<&str>,
        cf_score: f64,
    ) -> AffinityScore {
        let io = Self::interest_overlap(user_interests, candidate_interests);
        let lo = Self::language_overlap(user_langs, candidate_langs);
        let ia = Self::intent_alignment(user_intent, candidate_intent);

        let total = (self.interest_weight * io
            + self.language_weight * lo
            + self.intent_weight * ia
            + self.cf_weight * cf_score)
            .clamp(0.0, 1.0);

        AffinityScore {
            total,
            interest_overlap: io,
            language_overlap: lo,
            intent_alignment: ia,
            cf_score,
        }
    }
}

/// Lightweight collaborative filtering via co-like matrix.
///
/// Builds a sparse matrix: for each user pair (A, B), if A liked candidate C
/// and B also liked C, then A↔B have a co-like. When scoring candidate D for
/// user A, if B liked D and A↔B have co-likes, D gets a CF boost.
///
/// This is a simplified item-item CF adapted for dating:
/// "users who liked whom you liked also liked this person."
pub struct CoLikeMatrix {
    /// co_likes\[user_id\] = map of co-like-user_id → co-like count
    co_likes: HashMap<i32, HashMap<i32, u32>>,
    /// who_liked\[candidate_id\] = set of user_ids who liked this candidate
    who_liked: HashMap<i32, Vec<i32>>,
}

impl CoLikeMatrix {
    pub fn new() -> Self {
        Self {
            co_likes: HashMap::new(),
            who_liked: HashMap::new(),
        }
    }

    /// Build the co-like matrix from recent interaction data.
    /// Limits to recent interactions to keep the matrix fresh and bounded.
    pub async fn build(pool: &PgPool, days: i32, max_rows: i64) -> Self {
        #[derive(sqlx::FromRow)]
        struct LikeRow {
            user_id: i32,
            target_user_id: i32,
        }

        let likes = sqlx::query_as::<_, LikeRow>(
            r#"SELECT user_id, target_user_id
               FROM interaction_events
               WHERE event_type = 'like'
                 AND created_at > NOW() - ($1 || ' days')::interval
               ORDER BY created_at DESC
               LIMIT $2"#,
        )
        .bind(days)
        .bind(max_rows)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        let mut who_liked: HashMap<i32, Vec<i32>> = HashMap::new();
        for like in &likes {
            who_liked
                .entry(like.target_user_id)
                .or_default()
                .push(like.user_id);
        }

        // Build co-like counts: users who liked the same candidates
        let mut co_likes: HashMap<i32, HashMap<i32, u32>> = HashMap::new();
        for likers in who_liked.values() {
            if likers.len() < 2 || likers.len() > 500 {
                continue; // Skip too-popular or trivial candidates
            }
            for i in 0..likers.len() {
                for j in (i + 1)..likers.len() {
                    let a = likers[i];
                    let b = likers[j];
                    *co_likes.entry(a).or_default().entry(b).or_insert(0) += 1;
                    *co_likes.entry(b).or_default().entry(a).or_insert(0) += 1;
                }
            }
        }

        Self { co_likes, who_liked }
    }

    /// Compute CF score for a candidate for a given user.
    /// Score = fraction of the user's co-like partners who also liked this candidate.
    pub fn cf_score(&self, user_id: i32, candidate_id: i32) -> f64 {
        let user_colinks = match self.co_likes.get(&user_id) {
            Some(m) => m,
            None => return 0.0,
        };
        let candidate_likers = match self.who_liked.get(&candidate_id) {
            Some(v) => v,
            None => return 0.0,
        };

        if user_colinks.is_empty() {
            return 0.0;
        }

        // Weighted overlap: sum co-like counts for users who also liked this candidate
        let mut signal = 0.0;
        let mut total_weight = 0.0;
        for (co_user, &co_count) in user_colinks {
            let weight = (co_count as f64).sqrt(); // Diminishing returns on co-like count
            total_weight += weight;
            if candidate_likers.contains(co_user) {
                signal += weight;
            }
        }

        if total_weight == 0.0 { 0.0 } else { (signal / total_weight).clamp(0.0, 1.0) }
    }
}

impl Default for CoLikeMatrix {
    fn default() -> Self {
        Self::new()
    }
}

/// Fetch user interests and languages from the database as string arrays.
pub async fn fetch_user_tags(pool: &PgPool, user_id: i32) -> (Vec<String>, Vec<String>, Option<String>) {
    #[derive(sqlx::FromRow)]
    struct TagRow {
        interests: Option<serde_json::Value>,
        languages: Option<serde_json::Value>,
        looking_for: Option<String>,
    }

    let row = sqlx::query_as::<_, TagRow>(
        "SELECT interests, languages, looking_for FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    match row {
        Some(r) => {
            let interests = r
                .interests
                .and_then(|v| {
                    v.as_array().map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                })
                .unwrap_or_default();
            let languages = r
                .languages
                .and_then(|v| {
                    v.as_array().map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                })
                .unwrap_or_default();
            (interests, languages, r.looking_for)
        }
        None => (vec![], vec![], None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interest_overlap() {
        let a = vec!["hiking".into(), "cooking".into(), "music".into()];
        let b = vec!["cooking".into(), "music".into(), "travel".into()];
        let score = AffinityScorer::interest_overlap(&a, &b);
        // Jaccard: intersection=2, union=4 → 0.5
        assert!((score - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_interest_overlap_empty() {
        let a: Vec<String> = vec![];
        let b = vec!["cooking".into()];
        assert!(AffinityScorer::interest_overlap(&a, &b) < 0.01);
    }

    #[test]
    fn test_intent_alignment() {
        assert!((AffinityScorer::intent_alignment(Some("relationship"), Some("relationship")) - 1.0).abs() < 0.01);
        assert!((AffinityScorer::intent_alignment(Some("relationship"), Some("casual")) - 0.5).abs() < 0.01);
        assert!((AffinityScorer::intent_alignment(Some("relationship"), Some("friendship")) - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_affinity_score() {
        let scorer = AffinityScorer::default();
        let result = scorer.score(
            &["hiking".into(), "music".into()],
            &["music".into(), "travel".into()],
            &["english".into()],
            &["english".into(), "hindi".into()],
            Some("relationship"),
            Some("relationship"),
            0.5,
        );
        assert!(result.total > 0.3);
        assert!(result.total < 1.0);
        assert!((result.interest_overlap - 1.0 / 3.0).abs() < 0.01);
        assert!((result.language_overlap - 0.5).abs() < 0.01);
        assert!((result.intent_alignment - 1.0).abs() < 0.01);
    }
}
