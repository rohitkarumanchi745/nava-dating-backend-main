//! In-process ranking router.
//!
//! Maps an Objective + request context to a concrete ranker implementation
//! using JSON-driven rules. Inspired by Netflix's Switchboard, but scoped to
//! one process — no separate routing service, no Envoy, no cluster sharding.
//!
//! Today this only handles the `SpotsFeed` objective. Adding a new objective
//! means: define its trait, add a method on `RankingRouter`, extend
//! `RoutingRules` with a new field. No generics over trait objects.
//!
//! Selector precedence per request:
//!
//! 1. Active experiment cell (stable per user_id + experiment_id).
//! 2. Canary ramp (stable per user_id, percent of traffic).
//! 3. Default ranker.
//!
//! Optionally a shadow ranker runs concurrently; its result is logged, not used.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value as JsonValue;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::error::AppError;

// ---------------------------------------------------------------------------
// SpotsFeed objective: types
// ---------------------------------------------------------------------------

/// Minimal spot fields a ranker needs. Decouples rankers from the DB row type.
#[derive(Debug, Clone)]
pub struct SpotCandidate {
    pub id: i32,
    pub user_id: i32,
    pub title: Option<String>,
    pub original_url: Option<String>,
    pub poster_url: Option<String>,
    pub city: Option<String>,
    pub tags: Option<JsonValue>,
    pub created_at: Option<chrono::NaiveDateTime>,
    pub expires_at: Option<chrono::NaiveDateTime>,
    pub hls_url: Option<String>,
    pub hls_state: Option<String>,
}

/// Per-request context for SpotsFeed ranking.
#[derive(Debug, Clone)]
pub struct SpotsFeedCtx {
    pub user_id: i32,
    pub user_city: String,
    pub user_interests: Vec<String>,
    pub limit: usize,
}

/// Output of a ranker: spot + score, in input order.
#[derive(Debug, Clone)]
pub struct ScoredSpot {
    pub spot: SpotCandidate,
    pub score: f64,
}

/// Result returned to the handler. Includes which model was selected so the
/// handler can attach it to the response and to logs.
#[derive(Debug)]
pub struct RankResult<T> {
    pub ranked: Vec<T>,
    pub model_id: &'static str,
    pub shadow_model_id: Option<&'static str>,
    pub experiment_id: Option<u64>,
    pub experiment_cell: Option<u8>,
}

#[async_trait]
pub trait SpotsFeedRanker: Send + Sync {
    fn id(&self) -> &'static str;
    /// Score each candidate. Must return one score per input, in the same order.
    async fn score(
        &self,
        ctx: &SpotsFeedCtx,
        spots: &[SpotCandidate],
    ) -> Result<Vec<f64>, AppError>;
}

// ---------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone, Default)]
pub struct RoutingRules {
    #[serde(default)]
    pub spots_feed: ObjectiveRules,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ObjectiveRules {
    pub default: String,
    #[serde(default)]
    pub experiments: Vec<Experiment>,
    #[serde(default)]
    pub canary: Option<Canary>,
    #[serde(default)]
    pub shadow: Option<String>,
}

impl Default for ObjectiveRules {
    fn default() -> Self {
        Self {
            default: "spots_feed_heuristic".to_string(),
            experiments: Vec::new(),
            canary: None,
            shadow: None,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Experiment {
    pub id: u64,
    /// Map cell number -> ranker id. Cell selection is stable per user_id.
    pub cells: HashMap<u8, String>,
    #[serde(default = "default_true")]
    pub active: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Canary {
    pub ranker: String,
    /// 0..=100. Percent of users routed to the canary ranker.
    pub percent: u8,
}

fn default_true() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Stable bucketing
// ---------------------------------------------------------------------------

/// Splitmix64 — small, deterministic, stable across builds. Used to hash
/// (user_id, salt) into a uniform u64 for cell/canary assignment.
fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E3779B97F4A7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D049BB133111EB);
    x ^ (x >> 31)
}

fn bucket(user_id: i32, salt: u64, modulus: u64) -> u64 {
    let mixed = splitmix64((user_id as u64).wrapping_mul(0x100000001B3) ^ salt);
    mixed % modulus.max(1)
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub struct RankingRouter {
    spots_feed_rankers: HashMap<&'static str, Arc<dyn SpotsFeedRanker>>,
    rules: RwLock<RoutingRules>,
}

impl RankingRouter {
    pub fn new(
        spots_feed_rankers: Vec<Arc<dyn SpotsFeedRanker>>,
        rules: RoutingRules,
    ) -> Self {
        let mut map = HashMap::new();
        for r in spots_feed_rankers {
            map.insert(r.id(), r);
        }
        Self {
            spots_feed_rankers: map,
            rules: RwLock::new(rules),
        }
    }

    /// Replace rules atomically. Safe to call from an admin endpoint or a
    /// future file-watcher / Redis-pubsub reload path.
    pub async fn replace_rules(&self, rules: RoutingRules) {
        *self.rules.write().await = rules;
    }

    /// Load rules from a JSON file. Returns Ok even if the file is missing —
    /// that is treated as "use built-in defaults" so the server still boots
    /// in environments without the config mounted.
    pub async fn load_rules_from_file(&self, path: &Path) -> Result<bool, AppError> {
        if !path.exists() {
            info!("ranking rules file not found at {:?}, using defaults", path);
            return Ok(false);
        }
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|e| AppError::internal(format!("read ranking rules: {e}")))?;
        let parsed: RoutingRules = serde_json::from_slice(&bytes)
            .map_err(|e| AppError::internal(format!("parse ranking rules: {e}")))?;
        *self.rules.write().await = parsed;
        info!("ranking rules loaded from {:?}", path);
        Ok(true)
    }

    /// Public selector for SpotsFeed. Returns the chosen ranker id, the
    /// optional shadow ranker id, and any experiment metadata.
    async fn select_spots_feed(&self, user_id: i32) -> Selection {
        let rules = self.rules.read().await.spots_feed.clone();
        select(user_id, &rules)
    }

    /// Rank candidate spots end-to-end: select model, score, run shadow, sort, truncate.
    pub async fn rank_spots_feed(
        &self,
        ctx: &SpotsFeedCtx,
        spots: Vec<SpotCandidate>,
    ) -> Result<RankResult<ScoredSpot>, AppError> {
        let sel = self.select_spots_feed(ctx.user_id).await;

        let primary = self
            .spots_feed_rankers
            .get(sel.primary.as_str())
            .ok_or_else(|| {
                AppError::internal(format!(
                    "spots_feed ranker '{}' not registered",
                    sel.primary
                ))
            })?
            .clone();

        // Fire-and-forget shadow run. Don't block primary, don't fail request on shadow error.
        if let Some(shadow_id) = sel.shadow.clone() {
            if let Some(shadow) = self.spots_feed_rankers.get(shadow_id.as_str()).cloned() {
                let ctx_clone = ctx.clone();
                let spots_clone = spots.clone();
                tokio::spawn(async move {
                    match shadow.score(&ctx_clone, &spots_clone).await {
                        Ok(scores) => debug!(
                            shadow = %shadow.id(),
                            n = scores.len(),
                            "shadow ranker completed"
                        ),
                        Err(e) => warn!(shadow = %shadow.id(), error = %e, "shadow ranker failed"),
                    }
                });
            } else {
                warn!(
                    "shadow ranker '{}' referenced in rules but not registered",
                    shadow_id
                );
            }
        }

        let scores = primary.score(ctx, &spots).await?;
        if scores.len() != spots.len() {
            return Err(AppError::internal(format!(
                "ranker '{}' returned {} scores for {} candidates",
                primary.id(),
                scores.len(),
                spots.len()
            )));
        }

        let mut paired: Vec<ScoredSpot> = spots
            .into_iter()
            .zip(scores.into_iter())
            .map(|(spot, score)| ScoredSpot { spot, score })
            .collect();

        paired.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        paired.truncate(ctx.limit);

        let model_id = primary.id();
        let shadow_model_id = sel
            .shadow
            .as_deref()
            .and_then(|s| self.spots_feed_rankers.get(s).map(|r| r.id()));

        Ok(RankResult {
            ranked: paired,
            model_id,
            shadow_model_id,
            experiment_id: sel.experiment_id,
            experiment_cell: sel.experiment_cell,
        })
    }
}

#[derive(Debug, Clone)]
struct Selection {
    primary: String,
    shadow: Option<String>,
    experiment_id: Option<u64>,
    experiment_cell: Option<u8>,
}

fn select(user_id: i32, rules: &ObjectiveRules) -> Selection {
    // 1. First active experiment wins (matches Switchboard cell-rules semantics).
    for exp in &rules.experiments {
        if !exp.active || exp.cells.is_empty() {
            continue;
        }
        let mut cell_keys: Vec<u8> = exp.cells.keys().copied().collect();
        cell_keys.sort_unstable();
        let idx = bucket(user_id, exp.id, cell_keys.len() as u64) as usize;
        let cell = cell_keys[idx];
        if let Some(ranker) = exp.cells.get(&cell) {
            return Selection {
                primary: ranker.clone(),
                shadow: rules.shadow.clone(),
                experiment_id: Some(exp.id),
                experiment_cell: Some(cell),
            };
        }
    }

    // 2. Canary ramp.
    if let Some(canary) = &rules.canary {
        let pct = canary.percent.min(100) as u64;
        if pct > 0 && bucket(user_id, 0xC0DE_CAFE, 100) < pct {
            return Selection {
                primary: canary.ranker.clone(),
                shadow: rules.shadow.clone(),
                experiment_id: None,
                experiment_cell: None,
            };
        }
    }

    // 3. Default.
    Selection {
        primary: rules.default.clone(),
        shadow: rules.shadow.clone(),
        experiment_id: None,
        experiment_cell: None,
    }
}

// ---------------------------------------------------------------------------
// Adapter from the DB row type used by handlers.
// ---------------------------------------------------------------------------

impl From<crate::models::SpotFullRow> for SpotCandidate {
    fn from(r: crate::models::SpotFullRow) -> Self {
        Self {
            id: r.id,
            user_id: r.user_id,
            title: r.title,
            original_url: r.original_url,
            poster_url: r.poster_url,
            city: r.city,
            tags: r.tags,
            created_at: r.created_at,
            expires_at: r.expires_at,
            hls_url: r.hls_url,
            hls_state: r.hls_state,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn rules_with_default(default: &str) -> ObjectiveRules {
        ObjectiveRules {
            default: default.to_string(),
            experiments: Vec::new(),
            canary: None,
            shadow: None,
        }
    }

    #[test]
    fn default_is_used_when_no_experiment_or_canary() {
        let rules = rules_with_default("baseline");
        let s = select(42, &rules);
        assert_eq!(s.primary, "baseline");
        assert_eq!(s.experiment_id, None);
    }

    #[test]
    fn experiment_assignment_is_deterministic_per_user() {
        let mut cells = HashMap::new();
        cells.insert(1, "a".to_string());
        cells.insert(2, "b".to_string());
        cells.insert(3, "c".to_string());
        let rules = ObjectiveRules {
            default: "baseline".to_string(),
            experiments: vec![Experiment {
                id: 12345,
                cells,
                active: true,
            }],
            canary: None,
            shadow: None,
        };
        let s1 = select(7, &rules);
        let s2 = select(7, &rules);
        assert_eq!(s1.primary, s2.primary);
        assert_eq!(s1.experiment_cell, s2.experiment_cell);
        assert_eq!(s1.experiment_id, Some(12345));
    }

    #[test]
    fn experiment_cells_are_balanced_across_users() {
        let mut cells = HashMap::new();
        cells.insert(1, "a".to_string());
        cells.insert(2, "b".to_string());
        let rules = ObjectiveRules {
            default: "baseline".to_string(),
            experiments: vec![Experiment {
                id: 99,
                cells,
                active: true,
            }],
            canary: None,
            shadow: None,
        };
        let mut a = 0;
        let mut b = 0;
        for uid in 0..10_000 {
            match select(uid, &rules).primary.as_str() {
                "a" => a += 1,
                "b" => b += 1,
                _ => panic!("unexpected ranker"),
            }
        }
        // Splitmix64 over a tight range should split close to 50/50.
        let diff = (a as i64 - b as i64).abs();
        assert!(diff < 500, "imbalanced: a={} b={}", a, b);
    }

    #[test]
    fn inactive_experiments_are_skipped() {
        let mut cells = HashMap::new();
        cells.insert(1, "exp".to_string());
        let rules = ObjectiveRules {
            default: "baseline".to_string(),
            experiments: vec![Experiment {
                id: 1,
                cells,
                active: false,
            }],
            canary: None,
            shadow: None,
        };
        assert_eq!(select(1, &rules).primary, "baseline");
    }

    #[test]
    fn canary_routes_approximate_percent() {
        let rules = ObjectiveRules {
            default: "baseline".to_string(),
            experiments: Vec::new(),
            canary: Some(Canary {
                ranker: "canary".to_string(),
                percent: 10,
            }),
            shadow: None,
        };
        let mut canary_count = 0;
        for uid in 0..10_000 {
            if select(uid, &rules).primary == "canary" {
                canary_count += 1;
            }
        }
        // 10% target on 10k users; allow ±2 percentage points.
        assert!(
            (800..=1200).contains(&canary_count),
            "canary count {} out of expected band",
            canary_count
        );
    }

    #[test]
    fn shadow_id_is_propagated_in_default_path() {
        let rules = ObjectiveRules {
            default: "baseline".to_string(),
            experiments: Vec::new(),
            canary: None,
            shadow: Some("shadow_v1".to_string()),
        };
        assert_eq!(select(1, &rules).shadow.as_deref(), Some("shadow_v1"));
    }

    #[test]
    fn rules_json_round_trip() {
        let json = r#"{
            "spots_feed": {
                "default": "spots_feed_heuristic",
                "experiments": [
                    { "id": 12345, "cells": { "1": "spots_feed_heuristic", "2": "spots_feed_recency_only" } }
                ],
                "canary": { "ranker": "spots_feed_recency_only", "percent": 5 },
                "shadow": "spots_feed_recency_only"
            }
        }"#;
        let parsed: RoutingRules = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.spots_feed.default, "spots_feed_heuristic");
        assert_eq!(parsed.spots_feed.experiments.len(), 1);
        assert_eq!(parsed.spots_feed.experiments[0].cells.len(), 2);
        assert_eq!(parsed.spots_feed.canary.as_ref().unwrap().percent, 5);
    }
}
