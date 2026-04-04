// ============================================================================
// ToolSearch — A Built-in Tool for Dynamic Tool Discovery
//
// ToolSearch is itself a Tool. When the LLM needs a deferred tool, it calls
// ToolSearch, which returns the full schema. The LLM can then invoke the tool.
//
// This creates a beautiful recursion:
//   - ToolSearch is a tool that helps the LLM find other tools
//   - It's always resident (never deferred — you can't search for search)
//   - Its output is a JSON schema that the LLM uses to construct valid calls
//
// From Claude Code's perspective, this is "giving the LLM a dictionary."
// The dictionary's table of contents (tool names) is always visible.
// The LLM looks up entries (full schemas) only when it needs them.
// ============================================================================

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::common::{ToolContext, ToolOutput};
use crate::systems::tools::definition::Tool;
use crate::systems::tools::registry::ToolRegistry;

/// The ToolSearch built-in tool.
///
/// Holds a reference to the registry it searches.
/// Always resident — it cannot be deferred (that would be circular).
pub struct ToolSearchTool {
    registry: Arc<ToolRegistry>,
}

impl ToolSearchTool {
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        "ToolSearch"
    }

    fn description(&self) -> &str {
        "Fetch full schema definitions for deferred tools so they can be called."
    }

    fn prompt(&self) -> String {
        r#"Fetches full schema definitions for deferred tools so they can be called.

Deferred tools appear by name in <system-reminder> messages. Until fetched, only the name is known — there is no parameter schema, so the tool cannot be invoked. This tool takes a query, matches it against the deferred tool list, and returns the matched tools' complete definitions.

Query forms:
- "select:Read,Edit,Grep" — fetch these exact tools by name
- "notebook jupyter" — keyword search, up to max_results best matches
- "+slack send" — require "slack" in the name, rank by remaining terms"#
            .to_string()
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query. Use 'select:Name1,Name2' for exact match, keywords for search, '+prefix rest' for prefix-constrained search."
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return.",
                    "default": 5
                }
            },
            "required": ["query"]
        })
    }

    /// ToolSearch is always resident — never deferred.
    fn should_defer(&self) -> bool {
        false
    }

    /// ToolSearch is read-only and stateless — safe to run concurrently.
    fn is_concurrent(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> ToolOutput {
        let query = input["query"].as_str().unwrap_or("");
        let max_results = input["max_results"].as_u64().unwrap_or(5) as usize;

        if query.is_empty() {
            return ToolOutput::error("query is required");
        }

        let results = self.registry.search(query, max_results);

        if results.is_empty() {
            return ToolOutput::success(format!(
                "No tools found matching '{query}'. Registry has {} tools total. Available deferred tools: {}",
                self.registry.len(),
                self.registry.deferred_tool_names().join(", ")
            ));
        }

        // Format results as tool definitions the LLM can parse.
        // Include the full prompt (long description) when available so the LLM
        // has complete usage guidance before invoking.
        let formatted: Vec<String> = results
            .iter()
            .map(|t| {
                let prompt = self
                    .registry
                    .get(&t.name)
                    .map(|tool| tool.prompt())
                    .filter(|p| !p.is_empty())
                    .unwrap_or_else(|| t.description.clone());
                format!(
                    "Tool: {}\nDescription: {}\nUsage: {}\nInput Schema: {}",
                    t.name,
                    t.description,
                    prompt,
                    serde_json::to_string_pretty(&t.input_schema).unwrap_or_default()
                )
            })
            .collect();

        ToolOutput::success(formatted.join("\n\n---\n\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::tools::registry::ToolRegistry;

    // ToolSearchTool is always resident.
    #[test]
    fn test_tool_search_never_deferred() {
        let registry = Arc::new(ToolRegistry::new(vec![]));
        let tool = ToolSearchTool::new(registry);
        assert!(!tool.should_defer());
        assert!(tool.is_concurrent());
    }
}
