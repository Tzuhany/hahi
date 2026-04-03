// MemorySearch — LLM explicitly searches long-term memory.
//
// Complements automatic recall: the LLM uses this when it needs something
// specific that wasn't surfaced by the per-turn recall pipeline, or when
// it wants to check if it already knows something before writing.
//
// Runs RRF over ALL non-retired memories (including pinned kinds).
// Returns up to 5 results with full body content.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::common::{ToolContext, ToolInput, ToolOutput};
use crate::memory::ctx::MemoryCtx;
use crate::memory::embed::try_embed;
use crate::memory::types::Memory;
use crate::tool::definition::Tool;

pub struct MemorySearchTool(pub Arc<MemoryCtx>);

impl MemorySearchTool {
    pub fn new(ctx: Arc<MemoryCtx>) -> Self {
        Self(ctx)
    }
}

#[async_trait]
impl Tool for MemorySearchTool {
    fn name(&self) -> &str {
        "MemorySearch"
    }

    fn description(&self) -> &str {
        "Search long-term memory for specific information."
    }

    fn prompt(&self) -> String {
        "Searches all memories (including those not auto-recalled this turn) \
         using hybrid lexical + semantic search.\n\
         Returns up to 5 results with full content.\n\
         \n\
         Use when:\n\
         - You need something specific that isn't in the recalled memories\n\
         - You want to check if you already know something before writing\n\
         - The user asks about something you might have remembered previously"
            .to_string()
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "What to search for",
                    "minLength": 1
                }
            },
            "required": ["query"]
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

        let query = match ToolInput(&input).required_str("query") {
            Ok(q) => q.to_string(),
            Err(e) => return ToolOutput::error(e),
        };

        let embedding = try_embed(&self.0.embedder, &query).await;

        match self
            .0
            .store
            .memory_search(&self.0.agent_id, &query, embedding.as_deref())
            .await
        {
            Ok(memories) if memories.is_empty() => {
                ToolOutput::success("no matching memories found".to_string())
            }
            Ok(memories) => ToolOutput::success(format_results(&memories)),
            Err(e) => ToolOutput::error(format!("search failed: {e}")),
        }
    }
}

fn format_results(memories: &[Memory]) -> String {
    let sections: Vec<String> = memories
        .iter()
        .map(|m| {
            format!(
                "[{kind}] {name} (id: {id})\n{body}",
                kind = m.kind,
                name = m.name,
                id = m.id,
                body = m.body.trim(),
            )
        })
        .collect();

    format!(
        "Found {} memory/memories:\n\n{}",
        memories.len(),
        sections.join("\n\n---\n\n")
    )
}
