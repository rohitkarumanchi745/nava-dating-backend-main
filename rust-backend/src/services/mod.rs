//! Services module
//!
//! Business logic and external service integrations:
//! - Graph service: Recommendation graph queries via PostgreSQL CTEs
//! - Payments: Razorpay (India) + Stripe (Global)
//! - Ads: AdMob, Facebook, Unity monetization
//! - Ambassador: Referral tracking and performance analytics
//! - Trust & Safety: Device fingerprinting, behavioral scoring, anomaly detection
//! - Moderation: Content moderation pipeline, text toxicity, spam, duplicate face, appeals
//! - Freshness: Profile decay scoring, edit rate limiting, anti-gaming
//! - Media Optimizer: Responsive image variants, format transcoding

pub mod graph_service;
pub mod graph;
pub mod payments;
pub mod ads;
pub mod ambassador;
pub mod trust_safety;
pub mod moderation;
pub mod freshness;
pub mod media_optimizer;
pub mod photo_pipeline;
pub mod swipe_service;
pub mod behavior_service;
pub mod graph_replay;
pub mod shadow_scoring;
// Agentic auto-matcher (reciprocal scoring + auto-match policy)
pub mod matchmaker;
// GNN graph-structure embeddings (offline-trained, served as a pairwise score)
pub mod gnn;
// Visual (photo) compatibility embeddings
pub mod visual;
