// ============================================================================
// Memory Recall
//
// Two separate queries per turn, cleanly separated:
//
//   1. recall_pinned()     — loads all identity + feedback memories unconditionally.
//                            These are always in context. Small, fast, O(n) where
//                            n is small (users rarely have > 20 pinned memories).
//
//   2. recall_conditional() — hybrid RRF over everything else.
//                             Fuses lexical (plainto_tsquery) and semantic (pgvector)
//                             rankings. Returns top-K sorted by rrf_score × importance.
//
// After recall, access_count and accessed_at are updated asynchronously
// so the lifecycle module can track which memories are valuable.
//
// If no embedder is configured, recall_conditional falls back to lexical-only.
// ============================================================================

use std::sync::Arc;

use anyhow::Result;

use crate::adapters::store::Store;
use crate::systems::memory::embed::{ArcEmbedder, try_embed};
use crate::systems::memory::types::RecallResult;

/// Number of recalled (non-pinned) memories to inject per turn.
/// Pinned memories are all loaded; this limits the conditional set.
const MAX_RECALLED: i64 = 8;

/// Run both recall queries and return a combined RecallResult.
///
/// `query` is the raw user message text. Used for both lexical search
/// and as the embedding input. May be empty (first turn, etc.) — the
/// queries handle that gracefully.
pub async fn recall(
    store: Arc<Store>,
    agent_id: &str,
    query: &str,
    embedder: &ArcEmbedder,
) -> Result<RecallResult> {
    // Generate embedding (None if no-op embedder or empty query).
    let embedding = if query.is_empty() {
        None
    } else {
        try_embed(embedder, query).await
    };

    // Run pinned and conditional queries concurrently.
    let (pinned, recalled) = tokio::try_join!(
        store.memory_recall_pinned(agent_id),
        store.memory_recall_conditional(agent_id, query, embedding.as_deref(), MAX_RECALLED),
    )?;

    // Asynchronously update access tracking for recalled memories.
    // We don't await this — it's a best-effort write that doesn't block the turn.
    if !recalled.is_empty() {
        let recalled_ids: Vec<String> = recalled.iter().map(|m| m.id.clone()).collect();
        let store_clone = Arc::clone(&store);
        tokio::spawn(async move {
            if let Err(e) = store_clone.memory_record_access(&recalled_ids).await {
                tracing::warn!(error = %e, "failed to update memory access counts");
            }
        });
    }

    Ok(RecallResult { pinned, recalled })
}
