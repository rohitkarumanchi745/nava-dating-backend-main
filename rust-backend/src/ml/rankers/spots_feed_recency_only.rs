//! Recency-only challenger ranker. Same shape as the heuristic ranker but
//! ignores city and interest signals — exists so an A/B test against the
//! heuristic ranker actually measures something.

use async_trait::async_trait;

use crate::error::AppError;
use crate::ml::router::{SpotCandidate, SpotsFeedCtx, SpotsFeedRanker};

pub struct SpotsFeedRecencyOnly;

#[async_trait]
impl SpotsFeedRanker for SpotsFeedRecencyOnly {
    fn id(&self) -> &'static str {
        "spots_feed_recency_only"
    }

    async fn score(
        &self,
        _ctx: &SpotsFeedCtx,
        spots: &[SpotCandidate],
    ) -> Result<Vec<f64>, AppError> {
        let now = chrono::Utc::now().naive_utc();
        let scores = spots
            .iter()
            .map(|s| match s.created_at {
                Some(created) => {
                    let age_hours = (now - created).num_hours() as f64;
                    1.0 / (1.0 + age_hours / 6.0)
                }
                None => 0.0,
            })
            .collect();
        Ok(scores)
    }
}
