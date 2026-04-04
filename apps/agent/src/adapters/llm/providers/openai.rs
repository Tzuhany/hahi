// ============================================================================
// OpenAI GPT Provider
//
// Implements LlmProvider for OpenAI's Chat Completions API.
// Handles streaming SSE, function calling (tool_calls), and response parsing.
//
// Key differences from Anthropic:
//   - System prompt goes in messages[0] with role "system", not a separate field
//   - Tool calls are in a `tool_calls` array on the assistant message, not content blocks
//   - Tool results are sent as separate messages with role "tool"
//   - SSE uses `chat.completion.chunk` format with no `event:` line ([DONE] terminates)
//   - No native "thinking" support
//
// SSE byte handling is delegated to llm::sse. This module only contains
// the OpenAI-specific OpenAiParser (stateful chunk → StreamEvent).
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

/// OpenAI GPT provider.
///
/// Stateless HTTP client. Safe to share across sub-agents via Arc.
pub struct OpenAIProvider {
    client: Client,
    api_key: String,
    base_url: String,
}

impl OpenAIProvider {
    const DEFAULT_BASE_URL: &str = "https://api.openai.com";

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
    ) -> OpenAIRequest {
        let mut api_messages = vec![ApiMessage {
            role: "system".to_string(),
            content: Some(system_prompt.to_string()),
            tool_calls: None,
            tool_call_id: None,
        }];

        for msg in messages {
            api_messages.extend(to_api_messages(msg));
        }

        let api_tools: Vec<ApiTool> = tools
            .iter()
            .map(|t| ApiTool {
                r#type: "function".to_string(),
                function: ApiFunctionDef {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.input_schema.clone(),
                },
            })
            .collect();

        OpenAIRequest {
            model: config.model.clone(),
            messages: api_messages,
            tools: if api_tools.is_empty() {
                None
            } else {
                Some(api_tools)
            },
            stream: true,
            stream_options: Some(StreamOptions {
                include_usage: true,
            }),
            max_tokens: Some(config.max_tokens),
            temperature: config.temperature,
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAIProvider {
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
            .post(format!("{}/v1/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI API error ({}): {}", status, body);
        }

        Ok(Box::pin(sse_stream(response, OpenAiParser::default())))
    }
}

// ============================================================================
// OpenAiParser — stateful chunk interpreter
// ============================================================================

/// Mutable state carried across SSE frames for the OpenAI chat completion protocol.
#[derive(Default)]
pub(crate) struct OpenAiParser {
    /// tool_call index → (id, name, accumulated json)
    tool_calls: std::collections::HashMap<u32, (String, String, String)>,
    done: bool,
    usage: TokenUsage,
}

impl SseEventParser for OpenAiParser {
    /// OpenAI sends no `event:` line — `event` is always "".
    /// `data` is either "[DONE]" or a `chat.completion.chunk` JSON.
    fn parse(&mut self, _event: &str, data: &str) -> Vec<StreamEvent> {
        if data == "[DONE]" {
            self.done = true;
            return vec![];
        }

        let Ok(chunk) = serde_json::from_str::<ChatCompletionChunk>(data) else {
            return vec![];
        };

        if let Some(usage) = chunk.usage {
            self.usage.input_tokens = usage.prompt_tokens.unwrap_or(0);
            self.usage.output_tokens = usage.completion_tokens.unwrap_or(0);
        }

        let Some(choice) = chunk.choices.into_iter().next() else {
            return vec![];
        };

        let mut events = Vec::new();

        if let Some(content) = choice.delta.content {
            if !content.is_empty() {
                events.push(StreamEvent::TextDelta { text: content });
            }
        }

        if let Some(tool_calls) = choice.delta.tool_calls {
            for tc in tool_calls {
                let index = tc.index;
                if let Some(id) = tc.id {
                    let name = tc
                        .function
                        .as_ref()
                        .and_then(|f| f.name.clone())
                        .unwrap_or_default();
                    self.tool_calls
                        .insert(index, (id.clone(), name.clone(), String::new()));
                    events.push(StreamEvent::ToolUseStart { id, name });
                }
                if let Some(ref func) = tc.function {
                    if let Some(ref args) = func.arguments {
                        if let Some(entry) = self.tool_calls.get_mut(&index) {
                            entry.2.push_str(args);
                            events.push(StreamEvent::ToolInputDelta {
                                id: entry.0.clone(),
                                json_chunk: args.clone(),
                            });
                        }
                    }
                }
            }
        }

        if let Some(finish_reason) = choice.finish_reason {
            let tool_ids: Vec<String> = self
                .tool_calls
                .values()
                .map(|(id, _, _)| id.clone())
                .collect();
            for id in tool_ids {
                events.push(StreamEvent::ToolUseEnd { id });
            }
            self.tool_calls.clear();

            let stop_reason = match finish_reason.as_str() {
                "stop" => StopReason::EndTurn,
                "tool_calls" => StopReason::ToolUse,
                "length" => StopReason::MaxTokens,
                _ => StopReason::EndTurn,
            };
            events.push(StreamEvent::MessageEnd {
                usage: self.usage.clone(),
                stop_reason,
            });
        }

        events
    }

    fn is_done(&self) -> bool {
        self.done
    }
}

// ============================================================================
// OpenAI API request types
// ============================================================================

#[derive(Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<ApiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ApiTool>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Serialize)]
struct ApiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ApiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Serialize)]
struct ApiToolCall {
    id: String,
    r#type: String,
    function: ApiToolCallFunction,
}

#[derive(Serialize)]
struct ApiToolCallFunction {
    name: String,
    arguments: String,
}

#[derive(Serialize)]
struct ApiTool {
    r#type: String,
    function: ApiFunctionDef,
}

#[derive(Serialize)]
struct ApiFunctionDef {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

/// Convert our unified Message to OpenAI's message format.
fn to_api_messages(msg: &Message) -> Vec<ApiMessage> {
    let role = match msg.role {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::System => "system",
    };

    if msg.role == Role::Assistant {
        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();

        for block in &msg.content {
            match block {
                ContentBlock::Text { text } => text_parts.push(text.clone()),
                ContentBlock::ToolUse { id, name, input } => {
                    tool_calls.push(ApiToolCall {
                        id: id.clone(),
                        r#type: "function".to_string(),
                        function: ApiToolCallFunction {
                            name: name.clone(),
                            arguments: serde_json::to_string(input).unwrap_or_else(|_| "{}".into()),
                        },
                    });
                }
                _ => {}
            }
        }

        let content = if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join(""))
        };
        let tc = if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        };
        return vec![ApiMessage {
            role: role.to_string(),
            content,
            tool_calls: tc,
            tool_call_id: None,
        }];
    }

    if msg.role == Role::User {
        let has_tool_results = msg
            .content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolResult { .. }));
        if has_tool_results {
            return msg
                .content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } => Some(ApiMessage {
                        role: "tool".to_string(),
                        content: Some(content.clone()),
                        tool_calls: None,
                        tool_call_id: Some(tool_use_id.clone()),
                    }),
                    ContentBlock::Text { text } => Some(ApiMessage {
                        role: "user".to_string(),
                        content: Some(text.clone()),
                        tool_calls: None,
                        tool_call_id: None,
                    }),
                    _ => None,
                })
                .collect();
        }
    }

    let text = msg
        .content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");

    vec![ApiMessage {
        role: role.to_string(),
        content: if text.is_empty() { None } else { Some(text) },
        tool_calls: None,
        tool_call_id: None,
    }]
}

// ============================================================================
// OpenAI SSE response types
// ============================================================================

#[derive(Deserialize)]
struct ChatCompletionChunk {
    choices: Vec<ChunkChoice>,
    usage: Option<ChunkUsage>,
}

#[derive(Deserialize)]
struct ChunkChoice {
    delta: ChunkDelta,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct ChunkDelta {
    content: Option<String>,
    tool_calls: Option<Vec<ChunkToolCall>>,
}

#[derive(Deserialize)]
struct ChunkToolCall {
    index: u32,
    id: Option<String>,
    function: Option<ChunkFunction>,
}

#[derive(Deserialize)]
struct ChunkFunction {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct ChunkUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_done_on_done_sentinel() {
        let mut parser = OpenAiParser::default();
        assert!(!parser.is_done());
        parser.parse("", "[DONE]");
        assert!(parser.is_done());
    }

    #[test]
    fn test_to_api_messages_simple_user() {
        let msg = Message::user("1", "hello");
        let api = to_api_messages(&msg);
        assert_eq!(api.len(), 1);
        assert_eq!(api[0].role, "user");
        assert_eq!(api[0].content.as_deref(), Some("hello"));
    }

    #[test]
    fn test_to_api_messages_assistant_with_tool_use() {
        let msg = Message::assistant(
            "1",
            vec![
                ContentBlock::Text {
                    text: "Let me search".into(),
                },
                ContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "web_search".into(),
                    input: serde_json::json!({"q": "rust"}),
                },
            ],
        );
        let api = to_api_messages(&msg);
        assert_eq!(api.len(), 1);
        assert_eq!(api[0].role, "assistant");
        assert!(api[0].tool_calls.is_some());
    }

    #[test]
    fn test_to_api_messages_tool_results_become_tool_role() {
        let msg = Message::tool_results(
            "1",
            vec![ContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: "result data".into(),
                is_error: false,
            }],
        );
        let api = to_api_messages(&msg);
        assert_eq!(api.len(), 1);
        assert_eq!(api[0].role, "tool");
        assert_eq!(api[0].tool_call_id.as_deref(), Some("t1"));
    }
}
