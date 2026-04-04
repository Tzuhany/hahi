// ============================================================================
// Anthropic Claude Provider
//
// Implements LlmProvider for Anthropic's Messages API.
// Handles streaming SSE, tool_use/tool_result, extended thinking,
// and prompt cache control.
//
// SSE event flow from Anthropic API:
//   message_start         → extract message id, model, usage
//   content_block_start   → new block: text / tool_use / thinking
//   content_block_delta   → incremental content for the current block
//   content_block_stop    → block complete
//   message_delta         → final stop_reason + usage
//   message_stop          → stream complete
//
// We convert this into our flat StreamEvent enum, erasing Anthropic-specific
// structure while preserving all information the agent loop needs.
//
// SSE byte handling is delegated to llm::sse. This module only contains
// the Anthropic-specific AnthropicParser (stateful frame → StreamEvent).
// ============================================================================

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::adapters::llm::provider::{LlmProvider, ProviderConfig, ToolDefinition};
use crate::adapters::llm::sse::{SseEventParser, sse_stream};
use crate::common::{ContentBlock, Message, Role, TokenUsage};
use crate::common::{StopReason, StreamEvent};

/// Anthropic Claude provider.
///
/// Stateless HTTP client. Safe to share across sub-agents via Arc.
/// API key is stored once; all requests use the same key.
pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    base_url: String,
}

impl AnthropicProvider {
    const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
    const API_VERSION: &str = "2023-06-01";

    pub fn new(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        let base_url = base_url.into();
        Self {
            client: Client::new(),
            api_key: api_key.into(),
            base_url: if base_url.is_empty() {
                Self::DEFAULT_BASE_URL.to_string()
            } else {
                base_url
            },
        }
    }

    fn build_request_body(
        &self,
        system_prompt: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
        config: &ProviderConfig,
    ) -> AnthropicRequest {
        let api_messages: Vec<ApiMessage> = messages
            .iter()
            .map(|msg| ApiMessage {
                role: match msg.role {
                    Role::User => "user".to_string(),
                    Role::Assistant => "assistant".to_string(),
                    Role::System => "user".to_string(),
                },
                content: msg
                    .content
                    .iter()
                    .filter_map(to_api_content_block)
                    .collect(),
            })
            .collect();

        let api_tools: Vec<ApiTool> = tools
            .iter()
            .map(|t| ApiTool {
                name: t.name.clone(),
                description: t.description.clone(),
                input_schema: t.input_schema.clone(),
            })
            .collect();

        let thinking = config
            .extensions
            .get("thinking")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let thinking_budget = config
            .extensions
            .get("thinking_budget_tokens")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32)
            .unwrap_or(8_000);

        AnthropicRequest {
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            system: system_prompt.to_string(),
            messages: api_messages,
            tools: if api_tools.is_empty() {
                None
            } else {
                Some(api_tools)
            },
            stream: true,
            temperature: config.temperature,
            thinking: thinking.map(|t| ThinkingConfig {
                r#type: t,
                budget_tokens: Some(thinking_budget),
            }),
        }
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn stream(
        &self,
        system_prompt: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
        config: &ProviderConfig,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = Result<StreamEvent, anyhow::Error>> + Send>>>
    {
        let body = self.build_request_body(system_prompt, messages, tools, config);

        let response = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", Self::API_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic API error ({}): {}", status, body);
        }

        Ok(Box::pin(sse_stream(response, AnthropicParser::default())))
    }
}

// ============================================================================
// AnthropicParser — stateful frame interpreter
// ============================================================================

/// Mutable state carried across SSE frames for the Anthropic event protocol.
#[derive(Default)]
pub(crate) struct AnthropicParser {
    current_block_type: Option<String>,
    current_tool_id: Option<String>,
    done: bool,
    usage: TokenUsage,
}

impl SseEventParser for AnthropicParser {
    fn parse(&mut self, event: &str, data: &str) -> Vec<StreamEvent> {
        match event {
            "message_start" => {
                if let Ok(msg) = serde_json::from_str::<MessageStartData>(data) {
                    if let Some(usage) = msg.message.usage {
                        self.usage.input_tokens = usage.input_tokens.unwrap_or(0);
                        self.usage.cache_read_tokens = usage.cache_read_input_tokens.unwrap_or(0);
                        self.usage.cache_creation_tokens =
                            usage.cache_creation_input_tokens.unwrap_or(0);
                    }
                }
                vec![]
            }

            "content_block_start" => {
                let Ok(block) = serde_json::from_str::<ContentBlockStart>(data) else {
                    return vec![];
                };
                match block.content_block.r#type.as_str() {
                    "text" => {
                        self.current_block_type = Some("text".into());
                        vec![]
                    }
                    "thinking" => {
                        self.current_block_type = Some("thinking".into());
                        vec![]
                    }
                    "tool_use" => {
                        self.current_block_type = Some("tool_use".into());
                        let id = block.content_block.id.unwrap_or_default();
                        let name = block.content_block.name.unwrap_or_default();
                        self.current_tool_id = Some(id.clone());
                        vec![StreamEvent::ToolUseStart { id, name }]
                    }
                    _ => vec![],
                }
            }

            "content_block_delta" => {
                let Ok(delta) = serde_json::from_str::<ContentBlockDelta>(data) else {
                    return vec![];
                };
                match delta.delta.r#type.as_str() {
                    "text_delta" => {
                        let text = delta.delta.text.unwrap_or_default();
                        vec![StreamEvent::TextDelta { text }]
                    }
                    "thinking_delta" => {
                        let text = delta.delta.thinking.unwrap_or_default();
                        vec![StreamEvent::ThinkingDelta { text }]
                    }
                    "input_json_delta" => {
                        let json_chunk = delta.delta.partial_json.unwrap_or_default();
                        let id = self.current_tool_id.clone().unwrap_or_default();
                        vec![StreamEvent::ToolInputDelta { id, json_chunk }]
                    }
                    _ => vec![],
                }
            }

            "content_block_stop" => {
                if self.current_block_type.as_deref() == Some("tool_use") {
                    let id = self.current_tool_id.take().unwrap_or_default();
                    self.current_block_type = None;
                    vec![StreamEvent::ToolUseEnd { id }]
                } else {
                    self.current_block_type = None;
                    vec![]
                }
            }

            "message_delta" => {
                let Ok(delta) = serde_json::from_str::<MessageDeltaData>(data) else {
                    return vec![];
                };
                if let Some(usage) = delta.usage {
                    self.usage.output_tokens = usage.output_tokens.unwrap_or(0);
                }
                let stop_reason = match delta.delta.stop_reason.as_deref() {
                    Some("end_turn") => StopReason::EndTurn,
                    Some("tool_use") => StopReason::ToolUse,
                    Some("max_tokens") => StopReason::MaxTokens,
                    _ => StopReason::EndTurn,
                };
                vec![StreamEvent::MessageEnd {
                    usage: self.usage.clone(),
                    stop_reason,
                }]
            }

            "message_stop" => {
                self.done = true;
                vec![]
            }

            "error" => vec![StreamEvent::Error {
                message: data.to_string(),
                is_retryable: false,
            }],

            _ => vec![],
        }
    }

    fn is_done(&self) -> bool {
        self.done
    }
}

// ============================================================================
// Anthropic API request types
// ============================================================================

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<ApiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ApiTool>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingConfig>,
}

#[derive(Serialize)]
struct ApiMessage {
    role: String,
    content: Vec<ApiContentBlock>,
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum ApiContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

#[derive(Serialize)]
struct ApiTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[derive(Serialize)]
struct ThinkingConfig {
    r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    budget_tokens: Option<u32>,
}

fn to_api_content_block(block: &ContentBlock) -> Option<ApiContentBlock> {
    match block {
        ContentBlock::Text { text } => Some(ApiContentBlock::Text { text: text.clone() }),
        ContentBlock::ToolUse { id, name, input } => Some(ApiContentBlock::ToolUse {
            id: id.clone(),
            name: name.clone(),
            input: input.clone(),
        }),
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => Some(ApiContentBlock::ToolResult {
            tool_use_id: tool_use_id.clone(),
            content: content.clone(),
            is_error: *is_error,
        }),
        ContentBlock::Thinking { .. }
        | ContentBlock::CompactBoundary { .. }
        | ContentBlock::Collapsed { .. } => None,
    }
}

// ============================================================================
// Anthropic SSE response types
// ============================================================================

#[derive(Deserialize)]
struct MessageStartData {
    message: MessageStartMessage,
}

#[derive(Deserialize)]
struct MessageStartMessage {
    usage: Option<ApiUsage>,
}

#[derive(Deserialize)]
struct ApiUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct ContentBlockStart {
    content_block: ContentBlockInfo,
}

#[derive(Deserialize)]
struct ContentBlockInfo {
    r#type: String,
    id: Option<String>,
    name: Option<String>,
}

#[derive(Deserialize)]
struct ContentBlockDelta {
    delta: DeltaPayload,
}

#[derive(Deserialize)]
struct DeltaPayload {
    r#type: String,
    text: Option<String>,
    thinking: Option<String>,
    partial_json: Option<String>,
}

#[derive(Deserialize)]
struct MessageDeltaData {
    delta: MessageDelta,
    usage: Option<ApiUsage>,
}

#[derive(Deserialize)]
struct MessageDelta {
    stop_reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_message_delta_tool_use_stop() {
        let mut parser = AnthropicParser::default();
        let events = parser.parse(
            "message_delta",
            r#"{"delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":42}}"#,
        );
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            StreamEvent::MessageEnd { stop_reason, usage }
            if *stop_reason == StopReason::ToolUse && usage.output_tokens == 42
        ));
    }

    #[test]
    fn test_parser_message_stop_sets_done() {
        let mut parser = AnthropicParser::default();
        assert!(!parser.is_done());
        parser.parse("message_stop", "{}");
        assert!(parser.is_done());
    }

    #[test]
    fn test_to_api_content_block_skips_internal_types() {
        let thinking = ContentBlock::Thinking {
            text: "hmm".to_string(),
        };
        assert!(to_api_content_block(&thinking).is_none());
    }

    #[test]
    fn test_to_api_content_block_converts_text() {
        let text = ContentBlock::Text {
            text: "hello".to_string(),
        };
        let result = to_api_content_block(&text);
        assert!(matches!(result, Some(ApiContentBlock::Text { text }) if text == "hello"));
    }
}
