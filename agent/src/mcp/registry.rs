// ============================================================================
// MCP Tool Adapter — Bridge MCP tools into our Tool trait
//
// Each MCP tool is wrapped in an McpToolAdapter that implements our Tool trait.
// This lets MCP tools be registered in the ToolRegistry alongside built-in tools.
//
// MCP tools are always deferred (should_defer = true) UNLESS the server
// config has always_load = true.
//
// Note: McpToolAdapter is instantiated by service.rs when MCP server configs
// are present. Dead code warnings are expected until that wiring is complete.
// ============================================================================
#![allow(dead_code)]

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::common::{ToolContext, ToolOutput};
use crate::mcp::client::McpClient;
use crate::tool::definition::Tool;

/// Wraps an MCP tool definition as our Tool trait.
pub struct McpToolAdapter {
    /// Prefixed name: "mcp__{server}__{tool}" to avoid collisions.
    name: String,
    description: String,
    input_schema: Value,
    server_name: String,
    original_tool_name: String,
    always_load: bool,

    /// Shared reference to the MCP client (for calling the tool).
    client: Arc<tokio::sync::Mutex<McpClient>>,
}

impl McpToolAdapter {
    pub fn new(
        server_name: &str,
        tool_name: &str,
        description: &str,
        input_schema: Value,
        always_load: bool,
        client: Arc<tokio::sync::Mutex<McpClient>>,
    ) -> Self {
        Self {
            name: format!("mcp__{server_name}__{tool_name}"),
            description: description.to_string(),
            input_schema,
            server_name: server_name.to_string(),
            original_tool_name: tool_name.to_string(),
            always_load,
            client,
        }
    }
}

#[async_trait]
impl Tool for McpToolAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn prompt(&self) -> String {
        format!(
            "MCP tool from '{}' server. Call this to {}",
            self.server_name, self.description
        )
    }

    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }

    /// MCP tools are deferred unless always_load is set.
    fn should_defer(&self) -> bool {
        !self.always_load
    }

    fn search_hint(&self) -> Option<&str> {
        Some(&self.server_name)
    }

    /// MCP tools are assumed concurrent-safe (external service handles isolation).
    fn is_concurrent(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> ToolOutput {
        let mut client = self.client.lock().await;
        match client.call_tool(&self.original_tool_name, input).await {
            Ok(result) => ToolOutput::success(result),
            Err(e) => ToolOutput::error(format!(
                "MCP tool '{}' failed: {}",
                self.original_tool_name, e
            )),
        }
    }
}
