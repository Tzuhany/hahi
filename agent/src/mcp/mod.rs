// ============================================================================
// MCP — Model Context Protocol Client
//
// Connects the agent to external tool providers via the MCP standard.
// MCP servers expose tools (functions), resources (data), and prompts.
//
// In our architecture:
//   - MCP tools are registered as DEFERRED tools (name-only in prompt)
//   - LLM discovers them via ToolSearch, same as any deferred tool
//   - Tools with _meta["anthropic/alwaysLoad"] = true bypass deferral
//
// Use cases (general-purpose, not code-specific):
//   - DevOps: connect to Kubernetes, Prometheus, PagerDuty
//   - Video: connect to render pipeline, asset management
//   - Data: connect to Snowflake, dbt, Airflow
//   - Comms: connect to Slack, email, calendar
//
// MCP spec: https://modelcontextprotocol.io
// ============================================================================

pub mod client;
pub mod registry;

#[allow(unused_imports)]
pub use client::McpClient;
#[allow(unused_imports)]
pub use registry::McpToolAdapter;
