//! Services module
//!
//! Business logic and external service integrations:
//! - Graph service: Neo4j dual-write operations
//! - Payments: Razorpay (India) + Stripe (Global)
//! - Ads: AdMob, Facebook, Unity monetization
//! - Ambassador: Referral tracking and performance analytics

pub mod graph_service;
pub mod neo4j_http;
pub mod payments;
pub mod ads;
pub mod ambassador;
