// ============================================================================
// Message — Persisted Conversation Turn
// ============================================================================

use chrono::{DateTime, Utc};

use crate::domain::ids::{MessageId, ThreadId};

/// Whether the message was written by the user or the agent.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
}

impl std::fmt::Display for MessageRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageRole::User => f.write_str("user"),
            MessageRole::Assistant => f.write_str("assistant"),
        }
    }
}

/// A persisted message in a thread.
///
/// Content is plain text. Structured content (tool calls, thinking) lives
/// in the agent's checkpoint — the session service sees only the final text.
#[derive(Debug, Clone)]
pub struct Message {
    pub id: MessageId,
    pub thread_id: ThreadId,
    pub role: MessageRole,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

impl Message {
    /// Create a new user message. `MessageId` is assigned here.
    pub fn user(thread_id: ThreadId, content: impl Into<String>) -> Self {
        Self {
            id: MessageId::new(),
            thread_id,
            role: MessageRole::User,
            content: content.into(),
            created_at: Utc::now(),
        }
    }

    /// Create a new assistant message. `MessageId` is assigned here.
    pub fn assistant(thread_id: ThreadId, content: impl Into<String>) -> Self {
        Self {
            id: MessageId::new(),
            thread_id,
            role: MessageRole::Assistant,
            content: content.into(),
            created_at: Utc::now(),
        }
    }
}
