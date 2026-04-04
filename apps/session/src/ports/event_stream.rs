use anyhow::Result;
use async_trait::async_trait;

use crate::domain::RunId;
use crate::infra::events::SessionEvent;

/// Outbound port: reads raw agent events from a stream backend (Redis).
///
/// The implementation subscribes to `results:{run_id}` and
/// translates Redis Stream entries into typed `SessionEvent` values.
#[async_trait]
pub trait AgentEventStream: Send + Sync {
    /// Subscribe to events for a run.
    ///
    /// `last_event_id`: resume from this Redis Stream ID (empty = from beginning).
    /// Returns a channel receiver that yields events as the agent produces them.
    async fn subscribe(
        &self,
        run_id: &RunId,
        last_event_id: &str,
    ) -> Result<tokio::sync::mpsc::Receiver<SessionEvent>>;
}
