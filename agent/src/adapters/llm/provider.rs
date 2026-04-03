// ============================================================================
// LLM Provider Trait
//
// The single abstraction that decouples the agent from any specific LLM.
// Every provider (Anthropic, OpenAI, Google, ...) implements this trait.
// The agent loop programs against it and never knows what's underneath.
//
// Design decisions:
//
//   Why one trait, not many?
//     Agent only needs one operation: "send messages, get streaming response."
//     Splitting into CompletionProvider + EmbeddingProvider + etc. adds
//     abstraction without value — embeddings are handled by memory-service,
//     not the agent loop.
//
//   Why return a Pin<Box<dyn Stream>> instead of impl Stream?
//     We need dynamic dispatch — the provider is selected at runtime from
//     config, not known at compile time. The Stream must be Send so it can
//     live across await points in the tokio runtime.
//
//   Why no &mut self?
//     Providers are stateless HTTP clients. Multiple agent loops (sub-agents)
//     share the same provider instance via Arc. Interior mutability (if needed
//     for token refresh) is handled inside the provider.
//
//   Where do provider-specific features go?
//     Anthropic's prompt cache, OpenAI's strict mode, etc. are passed through
//     ProviderConfig. The trait surface stays clean; providers read their own
//     config keys and ignore the rest.
// ============================================================================

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;

use crate::common::Message;
use crate::common::StreamEvent;

/// Definition of a tool, as presented to the LLM API.
///
/// Each provider converts this into its native format:
///   - Anthropic: `tools[].input_schema`
///   - OpenAI: `tools[].function.parameters`
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Provider-specific configuration.
///
/// Passed to `stream()` on every call. Providers read keys relevant to them
/// and ignore the rest. This avoids polluting the trait with provider-specific
/// parameters while still enabling full access to each provider's capabilities.
///
/// Examples:
///   - Anthropic: `thinking: "adaptive"`, `cache_control: true`
///   - OpenAI: `reasoning_effort: "high"`, `strict_mode: true`
#[derive(Debug, Clone, Default)]
pub struct ProviderConfig {
    pub model: String,
    pub max_tokens: u32,
    pub temperature: Option<f32>,

    /// Provider-specific extensions.
    /// Keys are provider-defined; unknown keys are silently ignored.
    pub extensions: std::collections::HashMap<String, serde_json::Value>,
}

/// The core abstraction for LLM providers.
///
/// One method. That's all the agent needs.
///
/// ```text
/// let stream = provider.stream(&system_prompt, &messages, &tools, &config).await?;
///
/// while let Some(event) = stream.next().await {
///     match event? {
///         StreamEvent::TextDelta { text } => { /* push to SSE */ },
///         StreamEvent::ToolUseStart { id, name } => { /* prepare tool */ },
///         // ...
///     }
/// }
/// ```
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Stream a response from the LLM.
    ///
    /// Sends the conversation (system prompt + messages + tool definitions)
    /// and returns a stream of events as the LLM generates its response.
    ///
    /// The stream ends with either:
    ///   - `StreamEvent::MessageEnd` (success)
    ///   - `StreamEvent::Error` (failure, possibly retryable)
    ///
    /// # Errors
    /// Returns `Err` for connection-level failures (DNS, TLS, timeout).
    /// API-level errors (rate limit, invalid request) are returned as
    /// `StreamEvent::Error` within the stream, allowing partial processing.
    async fn stream(
        &self,
        system_prompt: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
        config: &ProviderConfig,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = Result<StreamEvent, anyhow::Error>> + Send>>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_config_default() {
        let config = ProviderConfig::default();
        assert!(config.model.is_empty());
        assert_eq!(config.max_tokens, 0);
        assert!(config.temperature.is_none());
        assert!(config.extensions.is_empty());
    }

    #[test]
    fn test_tool_definition_construction() {
        let tool = ToolDefinition {
            name: "WebSearch".into(),
            description: "Search the web".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                },
                "required": ["query"]
            }),
        };
        assert_eq!(tool.name, "WebSearch");
    }
}
