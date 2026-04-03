// MemoryForget — LLM soft-deletes a specific memory.
//
// Memories are never hard-deleted — they're retired with a reason.
// The LLM uses this when:
//   - A memory is factually wrong
//   - A user preference has changed ("actually, emoji is fine now")
//   - A project reference is stale and cluttering the index
//
// The ID comes from the memory index (visible in the system prompt)
// or from a prior MemorySearch result.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::common::{ToolContext, ToolInput, ToolOutput};
use crate::systems::memory::ctx::MemoryCtx;
use crate::systems::tools::definition::Tool;

pub struct MemoryForgetTool(pub Arc<MemoryCtx>);

impl MemoryForgetTool {
    pub fn new(ctx: Arc<MemoryCtx>) -> Self {
        Self(ctx)
    }
}

#[async_trait]
impl Tool for MemoryForgetTool {
    fn name(&self) -> &str {
        "MemoryForget"
    }

    fn description(&self) -> &str {
        "Remove an outdated or incorrect memory."
    }

    fn prompt(&self) -> String {
        "Soft-deletes a memory by ID. The memory is retired, not permanently erased.\n\
         \n\
         Use when:\n\
         - A memory is factually wrong\n\
         - A user preference has changed\n\
         - A reference is stale and cluttering the index\n\
         \n\
         The memory ID is visible in the index ([kind] name  id: <uuid>) \
         or in MemorySearch results."
            .to_string()
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The memory ID to forget (UUID from the memory index or search results)",
                    "minLength": 1
                },
                "reason": {
                    "type": "string",
                    "description": "Why this memory is being forgotten (for audit trail)",
                    "minLength": 1
                }
            },
            "required": ["id", "reason"]
        })
    }

    fn should_defer(&self) -> bool {
        false
    }
    fn is_concurrent(&self) -> bool {
        false
    } // mutations are sequential

    async fn call(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        if ctx.cancel.is_cancelled() {
            return ToolOutput::error("cancelled");
        }

        let inp = ToolInput(&input);
        let id = match inp.required_str("id") {
            Ok(id) => id.to_string(),
            Err(e) => return ToolOutput::error(e),
        };
        let reason = match inp.required_str("reason") {
            Ok(r) => format!("agent_deleted: {r}"),
            Err(e) => return ToolOutput::error(e),
        };

        match self
            .0
            .store
            .memory_retire(&self.0.agent_id, &id, &reason)
            .await
        {
            Ok(()) => ToolOutput::success(format!("memory {id} forgotten")),
            Err(e) => ToolOutput::error(format!("failed to forget memory: {e}")),
        }
    }
}
