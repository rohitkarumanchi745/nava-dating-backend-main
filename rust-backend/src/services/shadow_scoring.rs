//! Shadow scoring (v1) — disciplined, bounded, observable.
//!
//! Implemented exactly to the design contract. No additions, no "clever"
//! extras. This module only *computes* scores — it does not reorder the
//! production candidate list. The discover handler logs both current_rank
//! and shadow_rank so we can measure displacement before trusting the
//! new signals.
//!
//! Formula:
//!     shadow_score = base_score
//!                  + 0.15 * graph_score
//!                  + 0.10 * behavior_score
//!                  + location_score
//!
//! Guardrails: missing features contribute 0.0, never 0.5. Reward positive
//! evidence; do not penalize absence of evidence.

/// Graph signal inputs (raw counts from graph queries).
#[derive(Debug, Clone, Copy, Default)]
pub struct GraphFeatures {
    pub fof_count: i32,
    pub mutual_like_neighbors: i32,
    pub missing: bool,
}

/// Behavior signal inputs (from user_behavior_profile).
#[derive(Debug, Clone, Copy, Default)]
pub struct BehaviorFeatures {
    pub peak_hour_gap: f64,       // circular 24h diff folded to 0..12
    pub activity_level_gap: f64,  // |viewer.sessions_per_day - cand.sessions_per_day|
    pub missing: bool,
}

/// Location signal inputs.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocationFeatures {
    pub same_city: bool,
    pub viewer_is_traveler: bool,
    pub candidate_is_traveler: bool,
    pub stale: bool,
}

/// Computed components for one candidate.
#[derive(Debug, Clone, Copy)]
pub struct ShadowComponents {
    pub graph_score: f64,
    pub behavior_score: f64,
    pub location_score: f64,
    pub shadow_score: f64,
}

/// graph_score in [0, 1]. Returns 0.0 when features are missing.
pub fn compute_graph_score(g: GraphFeatures) -> f64 {
    if g.missing { return 0.0; }
    let fof_part = 0.7 * (g.fof_count.min(5) as f64) / 5.0;
    let mln_part = 0.3 * (g.mutual_like_neighbors.min(3) as f64) / 3.0;
    (fof_part + mln_part).clamp(0.0, 1.0)
}

/// behavior_score in [0, 1]. Returns 0.0 when profile missing.
pub fn compute_behavior_score(b: BehaviorFeatures) -> f64 {
    if b.missing { return 0.0; }
    let peak_hour_compat = 1.0 - (b.peak_hour_gap / 12.0);
    let activity_compat = 1.0 - (b.activity_level_gap.min(5.0) / 5.0);
    0.6 * peak_hour_compat + 0.4 * activity_compat
}

/// location_score in {0.0, 0.05, 0.15}. No penalties in v1.
pub fn compute_location_score(l: LocationFeatures) -> f64 {
    if l.stale { return 0.0; }
    if l.same_city { return 0.15; }
    if l.viewer_is_traveler || l.candidate_is_traveler { return 0.05; }
    0.0
}

/// Apply the first-pass shadow formula.
pub fn shadow_formula(
    base_score: f64,
    graph_score: f64,
    behavior_score: f64,
    location_score: f64,
) -> f64 {
    base_score + 0.15 * graph_score + 0.10 * behavior_score + location_score
}

/// Compose everything for one candidate.
pub fn compute(
    base_score: f64,
    g: GraphFeatures,
    b: BehaviorFeatures,
    l: LocationFeatures,
) -> ShadowComponents {
    let graph_score = compute_graph_score(g);
    let behavior_score = compute_behavior_score(b);
    let location_score = compute_location_score(l);
    let shadow_score = shadow_formula(base_score, graph_score, behavior_score, location_score);
    ShadowComponents { graph_score, behavior_score, location_score, shadow_score }
}

/// Circular 24h hour gap folded to [0, 12].
pub fn circular_hour_gap(a: i16, b: i16) -> f64 {
    let raw = ((a - b).abs() % 24) as f64;
    raw.min(24.0 - raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_score_bounds() {
        assert_eq!(compute_graph_score(GraphFeatures { missing: true, ..Default::default() }), 0.0);
        assert_eq!(compute_graph_score(GraphFeatures { fof_count: 0, mutual_like_neighbors: 0, missing: false }), 0.0);
        assert!((compute_graph_score(GraphFeatures { fof_count: 10, mutual_like_neighbors: 10, missing: false }) - 1.0).abs() < 1e-9);
        assert!((compute_graph_score(GraphFeatures { fof_count: 7, mutual_like_neighbors: 5, missing: false }) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn behavior_score_bounds() {
        assert_eq!(compute_behavior_score(BehaviorFeatures { missing: true, ..Default::default() }), 0.0);
        let perfect = compute_behavior_score(BehaviorFeatures {
            peak_hour_gap: 0.0, activity_level_gap: 0.0, missing: false,
        });
        assert!((perfect - 1.0).abs() < 1e-9);
        let worst = compute_behavior_score(BehaviorFeatures {
            peak_hour_gap: 12.0, activity_level_gap: 10.0, missing: false,
        });
        assert!(worst.abs() < 1e-9);
    }

    #[test]
    fn location_score_exact_values() {
        assert_eq!(compute_location_score(LocationFeatures { stale: true, same_city: true, ..Default::default() }), 0.0);
        assert_eq!(compute_location_score(LocationFeatures { same_city: true, ..Default::default() }), 0.15);
        assert_eq!(compute_location_score(LocationFeatures { viewer_is_traveler: true, ..Default::default() }), 0.05);
        assert_eq!(compute_location_score(LocationFeatures { candidate_is_traveler: true, ..Default::default() }), 0.05);
        assert_eq!(compute_location_score(LocationFeatures::default()), 0.0);
    }

    #[test]
    fn shadow_formula_matches_spec() {
        let s = shadow_formula(0.8, 1.0, 1.0, 0.15);
        assert!((s - 1.20).abs() < 1e-9);
    }

    #[test]
    fn circular_hour_gap_wraps() {
        assert_eq!(circular_hour_gap(0, 0), 0.0);
        assert_eq!(circular_hour_gap(3, 15), 12.0);
        assert_eq!(circular_hour_gap(23, 1), 2.0);
        assert_eq!(circular_hour_gap(1, 23), 2.0);
        assert_eq!(circular_hour_gap(6, 18), 12.0);
    }
}
