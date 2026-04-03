// ============================================================================
// Checkpoint Types — Agent Resume Snapshot
//
// Everything the agent needs to resume execution for a thread.
// Stored in PG (durable) and Redis (hot cache, 24h TTL).
//
// Conversation ownership lives outside the agent. Checkpoints are execution
// snapshots only; they are not the source of truth for conversation metadata.
//
// Lives in common/ because it's referenced by store (persistence),
// core (loop state), and multi (fork operations).
// ============================================================================

use serde::{Deserialize, Serialize};

use crate::common::message::Message;

/// Everything the agent needs to resume a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub thread_id: String,
    pub compact_summary: Option<String>,
    pub recent_messages: Vec<Message>,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub compact_count: u32,

    /// If this checkpoint was forked from another thread.
    pub forked_from: Option<ForkOrigin>,

    /// Structured control request that paused execution.
    #[serde(default)]
    pub pending_control: Option<PendingControl>,
}

/// A structured control request that can resume a paused turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingControl {
    pub request_id: String,
    pub kind: String,
    pub payload: serde_json::Value,
}

/// Where a forked conversation came from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkOrigin {
    /// The original thread's ID.
    pub source_thread_id: String,
    /// The message ID where the fork happened.
    pub fork_point_message_id: String,
}
