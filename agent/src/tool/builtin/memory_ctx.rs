// MemoryCtx — shared context for all three memory tools.
//
// MemoryWrite, MemorySearch, and MemoryForget all need the same three things:
// a store handle, an embedder, and the agent's ID. Previously each tool
// held its own copy. Now they share a single Arc<MemoryCtx>.
//
// Build once in run.rs, clone the Arc into each tool constructor.

use std::sync::Arc;

use crate::infra::store::Store;
use crate::memory::embed::ArcEmbedder;

/// Shared context injected into every memory-related tool.
pub struct MemoryCtx {
    pub store: Arc<Store>,
    pub embedder: ArcEmbedder,
    /// Agent-scoped namespace for all memory operations.
    pub agent_id: String,
}

impl MemoryCtx {
    pub fn new(store: Arc<Store>, embedder: ArcEmbedder, agent_id: impl Into<String>) -> Arc<Self> {
        Arc::new(Self { store, embedder, agent_id: agent_id.into() })
    }
}
