//! Faithful port of the original inline scoring in `get_spots_feed`:
//!   +0.4 same-city, +0.3 * interest_overlap_ratio, +0.3 / (1 + age_h/6).
//! Kept identical so swapping the handler over to the router is a pure refactor.

use async_trait::async_trait;

use crate::error::AppError;
use crate::ml::router::{SpotCandidate, SpotsFeedCtx, SpotsFeedRanker};

pub struct SpotsFeedHeuristic;

#[async_trait]
impl SpotsFeedRanker for SpotsFeedHeuristic {
    fn id(&self) -> &'static str {
        "spots_feed_heuristic"
    }

    async fn score(
        &self,
        ctx: &SpotsFeedCtx,
        spots: &[SpotCandidate],
    ) -> Result<Vec<f64>, AppError> {
        let now = chrono::Utc::now().naive_utc();
        let user_city_lc = ctx.user_city.to_lowercase();

        let scores = spots
            .iter()
            .map(|s| {
                let mut score = 0.0;

                if let Some(ref city) = s.city {
                    if city.to_lowercase() == user_city_lc {
                        score += 0.4;
                    }
                }

                if let Some(ref tags) = s.tags {
                    if let Ok(tag_list) = serde_json::from_value::<Vec<String>>(tags.clone()) {
                        let overlap = tag_list
                            .iter()
                            .filter(|t| ctx.user_interests.contains(t))
                            .count();
                        score += 0.3 * (overlap as f64 / tag_list.len().max(1) as f64);
                    }
                }

                if let Some(created) = s.created_at {
                    let age_hours = (now - created).num_hours() as f64;
                    score += 0.3 * (1.0 / (1.0 + age_hours / 6.0));
                }

                score
            })
            .collect();

        Ok(scores)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx(city: &str, interests: Vec<&str>) -> SpotsFeedCtx {
        SpotsFeedCtx {
            user_id: 1,
            user_city: city.to_string(),
            user_interests: interests.into_iter().map(String::from).collect(),
            limit: 10,
        }
    }

    fn spot(id: i32, city: Option<&str>, tags: Option<Vec<&str>>, age_hours: i64) -> SpotCandidate {
        SpotCandidate {
            id,
            user_id: 99,
            title: None,
            original_url: None,
            poster_url: None,
            city: city.map(String::from),
            tags: tags.map(|t| json!(t)),
            created_at: Some(chrono::Utc::now().naive_utc() - chrono::Duration::hours(age_hours)),
            expires_at: None,
            hls_url: None,
            hls_state: None,
        }
    }

    #[tokio::test]
    async fn same_city_outranks_other_city() {
        let r = SpotsFeedHeuristic;
        let c = ctx("Brooklyn", vec![]);
        let spots = vec![spot(1, Some("Brooklyn"), None, 1), spot(2, Some("Austin"), None, 1)];
        let scores = r.score(&c, &spots).await.unwrap();
        assert!(scores[0] > scores[1]);
    }

    #[tokio::test]
    async fn interest_overlap_boosts_score() {
        let r = SpotsFeedHeuristic;
        let c = ctx("Brooklyn", vec!["climbing", "coffee"]);
        let spots = vec![
            spot(1, None, Some(vec!["climbing", "coffee"]), 1),
            spot(2, None, Some(vec!["fishing"]), 1),
        ];
        let scores = r.score(&c, &spots).await.unwrap();
        assert!(scores[0] > scores[1]);
    }

    #[tokio::test]
    async fn newer_outranks_older_when_other_signals_equal() {
        let r = SpotsFeedHeuristic;
        let c = ctx("Brooklyn", vec![]);
        let spots = vec![spot(1, None, None, 1), spot(2, None, None, 100)];
        let scores = r.score(&c, &spots).await.unwrap();
        assert!(scores[0] > scores[1]);
    }

    #[tokio::test]
    async fn returns_one_score_per_input() {
        let r = SpotsFeedHeuristic;
        let c = ctx("Brooklyn", vec![]);
        let spots = vec![spot(1, None, None, 1), spot(2, None, None, 2), spot(3, None, None, 3)];
        let scores = r.score(&c, &spots).await.unwrap();
        assert_eq!(scores.len(), 3);
    }
}
