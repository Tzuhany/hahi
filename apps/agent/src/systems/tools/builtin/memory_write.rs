// MemoryWrite — LLM writes a new memory entry.
//
// Flow:
//   1. Extract and validate input (policy.rs)
//   2. Try to embed the body (embedder, may be no-op)
//   3. Write to PG (dedup via content hash unique index)
//   4. Return status to LLM

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::common::{ToolContext, ToolInput, ToolOutput};
use crate::systems::memory::ctx::MemoryCtx;
use crate::systems::memory::embed::try_embed;
use crate::systems::memory::policy;
use crate::systems::memory::types::WriteRequest;
use crate::systems::tools::definition::Tool;

pub struct MemoryWriteTool(pub Arc<MemoryCtx>);

impl MemoryWriteTool {
    pub fn new(ctx: Arc<MemoryCtx>) -> Self {
        Self(ctx)
    }
}

#[async_trait]
impl Tool for MemoryWriteTool {
    fn name(&self) -> &str {
        "MemoryWrite"
    }

    fn description(&self) -> &str {
        "Save something to long-term memory."
    }

    fn prompt(&self) -> String {
        "Writes a new memory entry that persists across sessions.\n\
         \n\
         When to use:\n\
         - User corrects your behavior (kind: \"feedback\")\n\
         - You learn something durable about the user (kind: \"identity\")\n\
         - An important decision was made (kind: \"decision\")\n\
         - A reference to an external system came up (kind: \"reference\")\n\
         \n\
         When NOT to use:\n\
         - Information derivable from the codebase or docs\n\
         - Task-specific details that won't matter next session\n\
         - Anything already in your memory index\n\
         \n\
         Pinned kinds (always in context): \"identity\", \"feedback\"\n\
         Body limit: 500 chars for pinned, 2000 for others."
            .to_string()
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "description": "Category of this memory. Common: identity, feedback, decision, experience, reference, fact",
                    "minLength": 1
                },
                "name": {
                    "type": "string",
                    "description": "Short title shown in the memory index (3-8 words)",
                    "minLength": 1
                },
                "body": {
                    "type": "string",
                    "description": "Full memory content",
                    "minLength": 1
                },
                "ttl_days": {
                    "type": "integer",
                    "description": "Optional: days until this memory expires. Omit for permanent.",
                    "minimum": 1
                }
            },
            "required": ["kind", "name", "body"]
        })
    }

    fn should_defer(&self) -> bool {
        false
    }
    fn is_concurrent(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        if ctx.cancel.is_cancelled() {
            return ToolOutput::error("cancelled");
        }

        let inp = ToolInput(&input);
        let kind = match inp.required_str("kind") {
            Ok(k) => k.to_string(),
            Err(e) => return ToolOutput::error(e),
        };
        let name = match inp.required_str("name") {
            Ok(n) => n.to_string(),
            Err(e) => return ToolOutput::error(e),
        };
        let body = match inp.required_str("body") {
            Ok(b) => b.to_string(),
            Err(e) => return ToolOutput::error(e),
        };
        let ttl_days = inp.optional_u64("ttl_days").map(|d| d as u32);

        let validated = match policy::validate(WriteRequest {
            agent_id: self.0.agent_id.clone(),
            kind,
            name,
            body,
            ttl_days,
        }) {
            Ok(v) => v,
            Err(e) => return ToolOutput::error(e.to_string()),
        };

        let embed_text = format!("{} {}", validated.name, validated.body);
        let embedding = try_embed(&self.0.embedder, &embed_text).await;

        match self
            .0
            .store
            .memory_write(&validated, embedding.as_deref())
            .await
        {
            Ok(status) => ToolOutput::success(status.to_tool_output()),
            Err(e) => ToolOutput::error(format!("failed to write memory: {e}")),
        }
    }
}
