// ============================================================================
// StreamEvents Use Case
//
// Handles the server-streaming gRPC path:
//   Gateway calls StreamEvents(run_id, last_event_id)
//   → session subscribes to Redis Stream directly (no in-process hub)
//   → streams until the channel closes (run terminated or client disconnected)
//
// Redis XREAD with last_event_id handles resume natively — no replay logic needed.
// ============================================================================

use std::sync::Arc;

use anyhow::Result;

use crate::domain::ids::RunId;
use crate::infra::events::SessionEvent;
use crate::ports::event_stream::AgentEventStream;

pub struct StreamEventsInput {
    pub run_id: RunId,
    /// Redis Stream offset for resume. Empty means start from beginning.
    pub last_event_id: String,
}

/// Subscribe to a run's event stream directly via Redis.
///
/// Returns an `mpsc::Receiver` that yields events as the agent produces them.
/// Pass `last_event_id` to resume from a specific Redis Stream offset.
pub async fn subscribe(
    input: StreamEventsInput,
    event_stream: Arc<dyn AgentEventStream>,
) -> Result<tokio::sync::mpsc::Receiver<SessionEvent>> {
    event_stream
        .subscribe(&input.run_id, &input.last_event_id)
        .await
}
