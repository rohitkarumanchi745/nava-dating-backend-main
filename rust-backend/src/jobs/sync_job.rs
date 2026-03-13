// =============================================================================
// NAVA Platform - Background Sync Job (no-op)
// =============================================================================
// Neo4j has been removed.  PostgreSQL is the sole source of truth.
// This module is kept as a no-op stub so that any remaining references compile.
// =============================================================================

use crate::state::AppState;

/// No-op startup sync — nothing to sync without Neo4j.
pub async fn run_startup_sync(_state: &AppState) -> Result<(), String> {
    Ok(())
}
