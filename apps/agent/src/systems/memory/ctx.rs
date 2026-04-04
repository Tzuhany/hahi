// ============================================================================
// MemoryCtx — shared context for all memory tools
//
// Moved from systems/tools/builtin/memory_ctx.rs to systems/memory/ so MemoryEngine can
// construct it without a circular dependency (memory → tool → memory).
//
// All three memory tools (Write, Search, Forget) share one Arc<MemoryCtx>,
// built once per turn by MemoryEngine::prepare_turn.
// ============================================================================

use std::sync::Arc;

use crate::adapters::store::Store;
use crate::systems::memory::embed::ArcEmbedder;

/// Shared context injected into every memory-related tool.
pub struct MemoryCtx {
    pub store: Arc<Store>,
    pub embedder: ArcEmbedder,
    /// Agent-scoped namespace for all memory operations.
    pub agent_id: String,
}

impl MemoryCtx {
    pub fn new(store: Arc<Store>, embedder: ArcEmbedder, agent_id: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            store,
            embedder,
            agent_id: agent_id.into(),
        })
    }
}
