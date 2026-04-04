// ============================================================================
// Unified Stream Events
//
// When the LLM streams a response, the raw SSE frames are provider-specific.
// Each provider adapter converts its SSE events into these unified types.
//
// The agent loop consumes StreamEvents without knowing whether Claude or GPT
// is underneath. This is the seam where provider-specific behavior is erased.
//
// StreamEvent is intentionally flat (no nested enums-of-enums). Each variant
// carries exactly the data the consumer needs — no more, no less.
// The agent loop pattern-matches on these, and Rust ensures exhaustiveness.
// ============================================================================

use crate::common::token::TokenUsage;

/// A single event from the LLM's streaming response.
///
/// Yielded one at a time from `LlmProvider::stream`.
/// The agent loop processes these as they arrive:
///   - TextDelta / ThinkingDelta → push to event batcher → SSE to client
///   - ToolUseStart → register pending tool
///   - ToolInputDelta → accumulate partial JSON for tool input
///   - ToolUseEnd → submit tool to executor (may start while LLM still streams)
///   - MessageEnd → finalize, check if follow-up needed
///   - Error → trigger error recovery
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// A chunk of text output from the LLM.
    TextDelta { text: String },

    /// A chunk of the LLM's internal reasoning.
    /// Not all providers support this (Anthropic: extended thinking, OpenAI: reasoning).
    ThinkingDelta { text: String },

    /// The LLM has started a tool call.
    /// At this point we know the tool name but not yet the full input.
    /// The executor can begin preparing (e.g., validating the tool exists).
    ToolUseStart { id: String, name: String },

    /// A chunk of the tool's input JSON, streamed incrementally.
    /// Accumulated by the agent loop until ToolUseEnd arrives.
    ToolInputDelta { id: String, json_chunk: String },

    /// The tool call's input is fully streamed.
    /// The agent loop parses the accumulated JSON and submits to the executor.
    ToolUseEnd { id: String },

    /// The LLM has finished its response.
    /// Contains final usage statistics for billing and monitoring.
    MessageEnd {
        usage: TokenUsage,
        stop_reason: StopReason,
    },

    /// An error occurred during streaming.
    /// The agent loop decides whether to retry, recover, or surface to the user.
    Error { message: String, is_retryable: bool },
}

/// Why the LLM stopped generating.
///
/// Determines the agent loop's next action:
///   - EndTurn → no tool calls, conversation turn is complete
///   - ToolUse → tool calls present, execute and continue
///   - MaxTokens → output truncated, may need continuation
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// Natural end of response. No tool calls requested.
    EndTurn,

    /// Response contains tool_use blocks. Agent should execute and continue.
    ToolUse,

    /// Output was truncated because max_tokens was reached.
    /// Agent may inject a "continue" prompt and retry.
    MaxTokens,
}

impl std::fmt::Display for StopReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StopReason::EndTurn => write!(f, "end_turn"),
            StopReason::ToolUse => write!(f, "tool_use"),
            StopReason::MaxTokens => write!(f, "max_tokens"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stop_reason_display() {
        assert_eq!(StopReason::EndTurn.to_string(), "end_turn");
        assert_eq!(StopReason::ToolUse.to_string(), "tool_use");
        assert_eq!(StopReason::MaxTokens.to_string(), "max_tokens");
    }
}
