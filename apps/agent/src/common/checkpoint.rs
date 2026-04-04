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
///
/// Persisted in Redis (hot, 24h TTL) and PostgreSQL (durable).
/// The checkpoint is the agent's only source of truth for conversation state —
/// thread metadata (title, participants, timestamps) lives in the session service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Identifies which thread this checkpoint belongs to.
    pub thread_id: String,

    /// LLM-generated summary of compacted history, if L3 compression has run.
    /// Injected at the top of the system prompt when present.
    pub compact_summary: Option<String>,

    /// Messages after the last compact boundary (what the LLM will see).
    pub recent_messages: Vec<Message>,

    /// Cumulative token spend for this thread (billing + monitoring).
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,

    /// How many times L3 compaction has run. Used to generate unique boundary IDs.
    pub compact_count: u32,

    /// Set when this thread was forked from another.
    /// `None` for root threads.
    pub forked_from: Option<ForkOrigin>,

    /// A permission or plan-review request that paused execution mid-turn.
    ///
    /// When set, the run is in `requires_action` state. The `ResumeRun` gRPC call
    /// clears this field and continues where execution left off.
    ///
    /// `#[serde(default)]` ensures old checkpoints without this field deserialize
    /// as `None` rather than failing.
    #[serde(default)]
    pub pending_control: Option<PendingControl>,
}

/// A structured control request that paused a run, waiting for external input.
///
/// `kind` identifies the type of pause:
///   - `"permission"` — tool execution waiting for user approval
///   - `"plan_review"` — plan mode waiting for user plan approval
///
/// `payload` carries the kind-specific details (e.g., tool name and input for
/// permission requests). Deserialized by `kernel/control.rs` on resume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingControl {
    /// Correlation ID matching the `ControlRequest` event emitted to the client.
    pub request_id: String,

    /// Type of pause. One of: `"permission"`, `"plan_review"`.
    pub kind: String,

    /// Kind-specific payload. Schema depends on `kind`.
    pub payload: serde_json::Value,
}

/// Where a forked conversation came from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkOrigin {
    /// The source thread's ID.
    pub source_thread_id: String,
    /// The message ID at which the fork was taken.
    pub fork_point_message_id: String,
}
