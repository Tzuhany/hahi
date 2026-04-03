// ============================================================================
// Unified Message Types
//
// These types are the lingua franca of the agent — every module speaks them.
// Provider-specific formats (Anthropic's content blocks, OpenAI's tool_calls)
// are converted to/from these in the adapter layer (llm/providers/*.rs).
//
// Design choices:
//   - ContentBlock is an enum, not a trait object. We know all variants at
//     compile time, and pattern matching is the natural way to handle them.
//   - Message owns its data. Cloning is acceptable — messages are small
//     compared to the LLM context window, and clarity beats micro-optimization.
//   - CompactBoundary is a content variant, not a separate message type.
//     This keeps the message list homogeneous and simplifies serialization.
// ============================================================================

use serde::{Deserialize, Serialize};

/// The role of a message in the conversation.
///
/// Maps directly to the LLM API's role concept.
/// System messages are injected by the framework, never by the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    System,
}

/// A single message in the conversation history.
///
/// The fundamental unit of context. The agent loop accumulates these,
/// the compact system summarizes them, and the LLM API consumes them.
///
/// Each message has a unique `id` for tracking through the system
/// (checkpoint references, compact boundaries, UI rendering).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

/// A block of content within a message.
///
/// Messages are not plain text — they carry structured content:
/// text, reasoning traces, tool invocations, tool results, and
/// compaction boundaries. This enum makes illegal states unrepresentable:
/// you cannot accidentally put a ToolResult inside an assistant message's
/// content — the type system prevents it at the call site.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Plain text content.
    Text { text: String },

    /// LLM's internal reasoning (extended thinking / chain-of-thought).
    /// Displayed to the user as a collapsible "thinking" section.
    Thinking { text: String },

    /// LLM requests a tool invocation.
    /// The `id` links this to the corresponding `ToolResult`.
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    /// Result of a tool invocation, sent back to the LLM.
    /// The `tool_use_id` must match a previous `ToolUse.id`.
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },

    /// Marks the boundary of a context compaction (L3).
    /// Everything before this was summarized into `summary`.
    /// The agent loop only sends messages after the last boundary to the LLM.
    CompactBoundary { summary: String },

    /// A collapsed segment of messages (L2).
    /// Original messages are preserved in PG — this is reversible.
    /// The summary is rule-generated (no LLM call).
    Collapsed {
        folded_count: u32,
        first_message_id: String,
        last_message_id: String,
        summary: String,
    },
}

// ============================================================================
// Message constructors
//
// Named constructors for common message patterns.
// Prefer these over raw struct literals — they enforce invariants
// (e.g., a user message always has exactly one text block).
// ============================================================================

impl Message {
    /// Create a user message with text content.
    pub fn user(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role: Role::User,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    /// Create an assistant message from content blocks.
    /// Used when the LLM response is fully received (not streaming).
    pub fn assistant(id: impl Into<String>, content: Vec<ContentBlock>) -> Self {
        Self {
            id: id.into(),
            role: Role::Assistant,
            content,
        }
    }

    /// Create a user message containing tool results.
    /// The LLM API expects tool results as user-role messages.
    pub fn tool_results(id: impl Into<String>, results: Vec<ContentBlock>) -> Self {
        Self {
            id: id.into(),
            role: Role::User,
            content: results,
        }
    }

    /// Create a compact boundary marker.
    /// Inserted by the compact system to mark where old messages were summarized.
    pub fn compact_boundary(id: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role: Role::System,
            content: vec![ContentBlock::CompactBoundary {
                summary: summary.into(),
            }],
        }
    }

    /// Check if this message is a compact boundary.
    pub fn is_compact_boundary(&self) -> bool {
        self.content
            .iter()
            .any(|b| matches!(b, ContentBlock::CompactBoundary { .. }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_message_has_single_text_block() {
        let msg = Message::user("1", "hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content.len(), 1);
        assert!(matches!(&msg.content[0], ContentBlock::Text { text } if text == "hello"));
    }

    #[test]
    fn test_compact_boundary_detection() {
        let msg = Message::compact_boundary("1", "summary of prior conversation");
        assert!(msg.is_compact_boundary());

        let msg = Message::user("2", "hello");
        assert!(!msg.is_compact_boundary());
    }
}
