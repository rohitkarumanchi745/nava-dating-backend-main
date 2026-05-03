//! Concrete ranker implementations registered with `RankingRouter`.
//! Each ranker is small, pure-ish, and unit-testable in isolation.

pub mod spots_feed_heuristic;
pub mod spots_feed_recency_only;

pub use spots_feed_heuristic::SpotsFeedHeuristic;
pub use spots_feed_recency_only::SpotsFeedRecencyOnly;
