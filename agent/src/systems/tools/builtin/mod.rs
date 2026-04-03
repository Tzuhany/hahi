// ============================================================================
// Built-in Tools
//
// These tools are always registered in every agent run.
// They are not MCP tools — they're compiled directly into the binary.
//
//   Agent       — spawns sub-agents (general, explorer, planner)
//   Memory*     — read/write/forget persistent memories (share one MemoryCtx)
//   WebSearch   — search the web via external provider
//   WebFetch    — fetch a URL and return its content
// ============================================================================

mod agent;
mod memory_forget;
mod memory_search;
mod memory_write;
mod web_fetch;
mod web_search;

pub use agent::{AgentTool, SpawnFn};
pub use memory_forget::MemoryForgetTool;
pub use memory_search::MemorySearchTool;
pub use memory_write::MemoryWriteTool;
pub use web_fetch::WebFetchTool;
pub use web_search::WebSearchTool;
