// ============================================================================
// SessionEvent — Typed Execution Events
//
// Internal semantic model for events parsed from the agent's Redis Stream.
//
// Flow:
//   Agent (Redis Stream) → parse_entry() → SessionEvent → app layer
//
// The app layer (send_message.rs) consumes these events to:
//   - Accumulate TextDelta content into the Run's in-memory buffer
//   - Trigger DB writes on RunCompleted
//   - Transition Run status on RunFailed
//
// Later, the gRPC service projects SessionEvents into the external
// EventFrame transport protocol for client consumption.
// ============================================================================

/// Events parsed from the agent's Redis Stream `results:{run_id}`.
#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// Agent has started execution (initial `session_state_changed` event).
    RunStarted,

    /// A chunk of text output streamed from the LLM.
    /// Accumulated by the app layer into `Run::accumulated_content`.
    TextDelta { text: String },

    /// A chunk of the LLM's internal reasoning trace.
    ThinkingDelta { text: String },

    /// The LLM has invoked a tool — forwarded to clients for real-time display.
    ToolStart {
        id: String,
        name: String,
        input_preview: String,
    },

    /// A tool has completed — forwarded to clients.
    ToolResult {
        id: String,
        name: String,
        content: String,
        is_error: bool,
    },

    /// The agent's streaming turn has ended successfully.
    ///
    /// `input_tokens` and `output_tokens` come from the agent's `turn_end` event
    /// and reflect the token spend for this execution cycle.
    ///
    /// The assistant message content is **not** carried here — it is delivered
    /// via the completion channel in `finalize_run` after the DB write completes.
    RunCompleted {
        input_tokens: u32,
        output_tokens: u32,
    },

    /// The agent returned an error or was cancelled.
    RunFailed { reason: String },

    /// The agent is waiting for external input (permission approval or plan review).
    ControlRequested {
        request_id: String,
        /// Type of pause: `"permission"` or `"plan_review"`.
        kind: String,
        payload_json: String,
    },

    /// The agent compressed its context window.
    Compacted { pre_tokens: u64 },
}
