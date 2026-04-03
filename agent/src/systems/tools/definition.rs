// ============================================================================
// Tool Trait
//
// The interface every tool implements. Tools are the agent's hands —
// they connect the LLM's intentions to the outside world.
//
// Design philosophy (from Claude Code):
//   "The framework is an executor, not a thinker."
//   The LLM decides WHICH tool to call and with WHAT arguments.
//   The framework just runs it and feeds the result back.
//
// Two faces:
//   1. Description (for LLM): name, description, prompt, input_schema
//   2. Implementation (for framework): call() → ToolOutput
//
// Tools are Send + Sync (shared across sub-agents) and stateless
// (state lives in ToolContext).
//
// Data types (ToolOutput, ToolContext, Artifact, etc.) live in common/tool_types.rs.
// ============================================================================

use async_trait::async_trait;
use serde_json::Value;

use crate::common::{ToolContext, ToolOutput};

/// The trait every tool implements.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique name. This is how the LLM references the tool.
    fn name(&self) -> &str;

    /// One-line description for the tool listing.
    fn description(&self) -> &str;

    /// Detailed usage instructions. Loaded on demand via ToolSearch.
    fn prompt(&self) -> String;

    /// JSON Schema for input parameters.
    fn input_schema(&self) -> Value;

    /// Whether this tool should be deferred (name-only in prompt).
    fn should_defer(&self) -> bool {
        false
    }

    /// Short search hint for ToolSearch (3-10 words).
    fn search_hint(&self) -> Option<&str> {
        None
    }

    /// Whether this tool can safely run concurrently with other tools.
    /// Read-only → true. Side effects → false.
    #[allow(dead_code)]
    fn is_concurrent(&self) -> bool {
        true
    }

    /// Execute the tool.
    ///
    /// Input is pre-validated against input_schema by the executor.
    /// If validation failed, this method is NOT called — the LLM gets
    /// an error result with the validation message instead.
    async fn call(&self, input: Value, ctx: &ToolContext) -> ToolOutput;
}
