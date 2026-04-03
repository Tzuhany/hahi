// ============================================================================
// MCP Client — Model Context Protocol via rmcp SDK
//
// Uses the official `rmcp` crate (v1.3) instead of hand-rolled JSON-RPC.
// rmcp handles the full protocol: handshake, notifications, pagination,
// content types (text / image / resource), and error propagation.
//
// Transport: stdio only (spawns a child process).
// HTTP transport is not yet implemented — use stdio.
//
// Pagination: tools/list results are automatically collected across all
// cursor pages so callers always receive the complete tool list.
//
// The MCP subsystem is wired in from the gRPC adapter once MCP server configs
// are read from agent config.
// ============================================================================

use anyhow::{Context, Result};
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, PaginatedRequestParams};
use rmcp::transport::TokioChildProcess;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

// ============================================================================
// Public configuration types (unchanged API surface)
// ============================================================================

/// Configuration for connecting to an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Human-readable name (e.g., "kubernetes", "slack").
    pub name: String,

    /// Transport type.
    pub transport: McpTransport,

    /// Whether this server's tools should bypass deferral and be always loaded.
    pub always_load: bool,
}

/// Transport configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpTransport {
    /// Spawn a child process and communicate over stdin/stdout.
    Stdio { command: String, args: Vec<String> },

    /// HTTP transport — not yet implemented.
    Http {
        url: String,
        headers: std::collections::HashMap<String, String>,
    },
}

/// A tool definition received from an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

// ============================================================================
// McpClient
// ============================================================================

// Type alias for the connected rmcp service.
// `()` is the handler — clients don't implement server-side handlers.
type RmcpService = rmcp::service::RunningService<rmcp::RoleClient, ()>;

/// Client for a single MCP server.
///
/// Wrapped in `Arc<tokio::sync::Mutex<McpClient>>` by `McpToolAdapter` so
/// that `call_tool` (which requires exclusive access to the stdio pipe) can
/// be called from the `Tool` trait's `&self` context.
pub struct McpClient {
    config: McpServerConfig,
    service: Option<RmcpService>,
}

impl McpClient {
    /// Create a client (does NOT connect — call `connect()` first).
    pub fn new(config: McpServerConfig) -> Self {
        Self {
            config,
            service: None,
        }
    }

    /// Connect to the MCP server and return its full tool list.
    ///
    /// Performs the rmcp handshake automatically and collects all pages of
    /// the tool list (handles `next_cursor` pagination).
    pub async fn connect(&mut self) -> Result<Vec<McpToolDef>> {
        match self.config.transport.clone() {
            McpTransport::Stdio { command, args } => self.connect_stdio(&command, &args).await,
            McpTransport::Http { .. } => {
                anyhow::bail!(
                    "HTTP MCP transport is not yet implemented for server '{}'. \
                     Use stdio transport instead.",
                    self.config.name
                )
            }
        }
    }

    async fn connect_stdio(&mut self, command: &str, args: &[String]) -> Result<Vec<McpToolDef>> {
        let mut cmd = Command::new(command);
        cmd.args(args);
        let transport = TokioChildProcess::new(cmd).with_context(|| {
            format!(
                "failed to spawn MCP server '{}' (command: {})",
                self.config.name, command
            )
        })?;

        let service = ()
            .serve(transport)
            .await
            .with_context(|| format!("MCP handshake failed for server '{}'", self.config.name))?;

        // Collect all pages of tools (handles servers with many tools).
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let params = PaginatedRequestParams::default().with_cursor(cursor.clone());
            let page = service
                .list_tools(Some(params))
                .await
                .context("failed to list MCP tools")?;

            for tool in page.tools {
                tools.push(McpToolDef {
                    name: tool.name.to_string(),
                    description: tool.description.as_deref().unwrap_or("").to_string(),
                    input_schema: serde_json::Value::Object((*tool.input_schema).clone()),
                });
            }

            match page.next_cursor {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }

        tracing::info!(
            server = self.config.name.as_str(),
            tools = tools.len(),
            "MCP server connected"
        );

        self.service = Some(service);
        Ok(tools)
    }

    /// Call a tool on the MCP server and return its text output.
    ///
    /// Concatenates all `text` content blocks in the response.
    /// Non-text blocks (image, resource) are noted but not returned.
    pub async fn call_tool(&mut self, tool_name: &str, input: serde_json::Value) -> Result<String> {
        let service = self
            .service
            .as_mut()
            .context("MCP client not connected — call connect() first")?;

        let arguments = match input {
            serde_json::Value::Object(map) => Some(map),
            serde_json::Value::Null => None,
            other => {
                // Wrap non-object inputs so the schema validator in the MCP
                // server gets a proper object (best-effort).
                let mut map = serde_json::Map::new();
                map.insert("value".to_string(), other);
                Some(map)
            }
        };

        let mut params = CallToolRequestParams::new(tool_name.to_owned());
        if let Some(args) = arguments {
            params = params.with_arguments(args);
        }
        let result = service
            .call_tool(params)
            .await
            .with_context(|| format!("MCP tool call failed: '{tool_name}'"))?;

        // Collect text blocks; log non-text blocks for observability.
        let mut parts: Vec<String> = Vec::new();
        let mut non_text = 0usize;

        for block in &result.content {
            match &block.raw {
                rmcp::model::RawContent::Text(t) => parts.push(t.text.clone()),
                _ => non_text += 1,
            }
        }

        if non_text > 0 {
            tracing::debug!(
                tool = tool_name,
                non_text_blocks = non_text,
                "MCP tool returned non-text content blocks (ignored)"
            );
        }

        if result.is_error == Some(true) {
            anyhow::bail!(
                "MCP tool '{}' signalled an error: {}",
                tool_name,
                parts.join("\n")
            );
        }

        Ok(parts.join("\n"))
    }
}
