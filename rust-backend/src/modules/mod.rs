//! Domain modules — modular monolith boundaries.
//!
//! Each sub-module owns its routes, handlers, and business logic while sharing
//! the same process, database pool, and in-process event bus.

pub mod events;
pub mod notifications;
pub mod analytics;
