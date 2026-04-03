// ============================================================================
// MemoryEngine — Memory Subsystem Facade
//
// Single entry point for all memory operations in runtime assembly.
// Replaces 6 scattered function calls with 2 clean method calls:
//
//   let mem = engine.prepare_turn(agent_id, query).await;
//   // ... use mem.index_section, mem.recalled_section, mem.tools ...
//   engine.spawn_post_turn(agent_id, stats);
//
// MemoryEngine does NOT handle reflection (that belongs to runtime policy)
// but provides the should_reflect check so the caller knows when to trigger it.
// ============================================================================

use std::sync::Arc;

use crate::adapters::store::Store;
use crate::systems::memory::ctx::MemoryCtx;
use crate::systems::memory::embed::ArcEmbedder;
use crate::systems::memory::types::SessionStats;
use crate::systems::memory::{inject, lifecycle, recall, reflect};

/// Everything the system prompt and tool registry need for a single turn.
pub struct TurnMemory {
    /// Shared context for MemoryWrite, MemorySearch, MemoryForget tools.
    pub ctx: Arc<MemoryCtx>,

    /// Pre-formatted memory index for the system prompt (None if no memories yet).
    pub index_section: Option<String>,

    /// Pre-formatted recalled memories for the turn start (None if nothing recalled).
    pub recalled_section: Option<String>,

    /// Static write guidance injected into the system prompt.
    pub write_guidance: &'static str,
}

/// Facade over the memory subsystem.
///
/// Hold one per RunPipeline and share via Arc.
pub struct MemoryEngine {
    store: Arc<Store>,
    embedder: ArcEmbedder,
}

impl MemoryEngine {
    pub fn new(store: Arc<Store>, embedder: ArcEmbedder) -> Arc<Self> {
        Arc::new(Self { store, embedder })
    }

    /// Prepare all memory material needed for one agent turn.
    ///
    /// Runs recall + index lookup concurrently, then formats both for prompt injection.
    /// The returned `TurnMemory.ctx` is already wired for the 3 memory tools.
    pub async fn prepare_turn(&self, agent_id: &str, query: &str) -> TurnMemory {
        // Recall and index can run concurrently — they hit different PG queries.
        let (recall_result, memory_index) = tokio::join!(
            recall::recall(Arc::clone(&self.store), agent_id, query, &self.embedder),
            self.store.memory_list_index(agent_id),
        );

        let recall_result = recall_result.unwrap_or_default();
        let memory_index = memory_index.unwrap_or_default();

        TurnMemory {
            ctx: MemoryCtx::new(
                Arc::clone(&self.store),
                Arc::clone(&self.embedder),
                agent_id,
            ),
            index_section: inject::format_index(&memory_index),
            recalled_section: inject::format_recalled(&recall_result),
            write_guidance: inject::format_write_guidance(),
        }
    }

    /// Build the shared memory-tool context for one agent.
    pub fn tool_ctx(&self, agent_id: &str) -> Arc<MemoryCtx> {
        MemoryCtx::new(
            Arc::clone(&self.store),
            Arc::clone(&self.embedder),
            agent_id,
        )
    }

    /// Fire-and-forget post-turn maintenance.
    ///
    /// Runs memory lifecycle decay in the background.
    /// Does not handle reflection (caller is responsible — reflection needs the LLM).
    pub fn spawn_lifecycle(&self, agent_id: String) {
        let store = Arc::clone(&self.store);
        tokio::spawn(async move {
            lifecycle::run(store, &agent_id).await;
        });
    }

    /// Whether the agent should run a reflection pass after this turn.
    pub fn should_reflect(&self, stats: &SessionStats) -> bool {
        reflect::should_reflect(stats).should_reflect()
    }
}
