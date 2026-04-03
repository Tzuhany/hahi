// ============================================================================
// Memory Lifecycle
//
// Runs once per session end (async, does not block the response).
// Two mechanisms — no moving parts beyond two SQL UPDATE statements:
//
//   1. Retire stale memories
//      Condition: access_count = 0 AND created_at older than STALE_DAYS
//      Effect: soft-delete (retired_at + retired_reason = "stale")
//      Rationale: if a memory was never recalled in N days, it wasn't needed.
//      Pinned memories (identity, feedback) are exempt — they're always pinned,
//      not recalled, so access_count stays 0 even for useful ones.
//
//   2. Adjust importance
//      Decay:  importance *= DECAY_FACTOR  for memories not accessed recently
//      Boost:  importance = min(1.0, importance * BOOST_FACTOR) for hot memories
//      Rationale: importance drives RRF re-ranking. High-access memories
//      naturally surface more; neglected ones fade without being deleted.
//
// No compaction, no promotion pipelines.
// The LLM has MemoryWrite and MemoryForget for anything beyond this.
// ============================================================================

#![allow(dead_code)]

use std::sync::Arc;

use crate::infra::store::Store;

/// Days of zero access before a non-pinned memory is retired.
const STALE_DAYS: i64 = 30;

/// How much importance decays per lifecycle run for untouched memories.
const DECAY_FACTOR: f64 = 0.95;

/// How much importance grows per lifecycle run for recently-accessed memories.
const BOOST_FACTOR: f64 = 1.10;

/// Minimum access count to trigger a boost (avoids boosting on single flukes).
const BOOST_MIN_ACCESSES: i64 = 3;

/// Days within which an access counts as "recent" for boosting.
const BOOST_RECENT_DAYS: i64 = 7;

/// Run the full lifecycle for one agent.
///
/// Called at the end of each session, fire-and-forget from the caller's side.
/// Logs errors rather than propagating — lifecycle failures should never
/// affect the agent's response path.
pub async fn run(store: Arc<Store>, agent_id: &str) {
    let agent_id = agent_id.to_string();

    // Retire stale memories.
    match store.memory_retire_stale(&agent_id, STALE_DAYS).await {
        Ok(n) if n > 0 => tracing::info!(agent_id, retired = n, "retired stale memories"),
        Ok(_) => {}
        Err(e) => tracing::warn!(agent_id, error = %e, "failed to retire stale memories"),
    }

    // Decay importance of untouched memories.
    match store.memory_decay_importance(&agent_id, DECAY_FACTOR).await {
        Ok(n) if n > 0 => tracing::debug!(agent_id, updated = n, "decayed memory importance"),
        Ok(_) => {}
        Err(e) => tracing::warn!(agent_id, error = %e, "failed to decay memory importance"),
    }

    // Boost importance of hot memories.
    match store
        .memory_boost_importance(
            &agent_id,
            BOOST_FACTOR,
            BOOST_MIN_ACCESSES,
            BOOST_RECENT_DAYS,
        )
        .await
    {
        Ok(n) if n > 0 => tracing::debug!(agent_id, updated = n, "boosted memory importance"),
        Ok(_) => {}
        Err(e) => tracing::warn!(agent_id, error = %e, "failed to boost memory importance"),
    }
}
