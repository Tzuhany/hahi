// ============================================================================
// EventBus — In-Process MPMC Event Channel
//
// Bridges the agent loop (producer) with downstream consumers (Redis writer,
// metrics, audit log, tests) using a tokio unbounded channel.
//
// Design decisions:
//   - UnboundedSender: emit() is sync and infallible (never drops).
//     The channel has no capacity limit; backpressure would stall LLM streaming.
//     In practice, events are bounded by the context window (≤200K tokens).
//
//   - Multiple producers: mpsc::Sender is Clone. Sub-agents call
//     bus.child_bus(task_id) which returns a bus whose events are selectively
//     forwarded to the parent (tool start/result only, not text deltas).
//
//   - Multiple consumers: EventBus::new() returns a Receiver. Callers may
//     fan out by spawning additional consumer tasks each holding a clone of
//     the underlying sender (see child_bus pattern).
//
// Wiring in run.rs:
//   let (bus, rx) = EventBus::new();
//   tokio::spawn(redis_consumer(rx, store, thread_id));   // consumer 1
//   run_loop(&config, &LoopRuntime { bus, .. }).await;    // producers
// ============================================================================

use tokio::sync::mpsc;

use crate::core::r#loop::LoopEvent;

/// In-process event bus. Cheap to clone (wraps an Arc-backed unbounded sender).
#[derive(Clone)]
pub struct EventBus {
    tx: mpsc::UnboundedSender<LoopEvent>,
}

impl EventBus {
    /// Create a new bus. Returns the bus (for producers) and the receiver
    /// (for the consumer task). Spawn at least one consumer task or the
    /// channel will accumulate events indefinitely.
    pub fn new() -> (Self, mpsc::UnboundedReceiver<LoopEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx }, rx)
    }

    /// Emit an event. Sync, non-blocking, infallible while any consumer lives.
    pub fn emit(&self, event: LoopEvent) {
        // Only fails if the receiver is dropped (process is shutting down).
        let _ = self.tx.send(event);
    }

    /// Create a child bus for a sub-agent.
    ///
    /// The child bus forwards a filtered subset of events to this bus with
    /// IDs prefixed by `task_id`, so the parent's event stream reflects
    /// sub-agent tool activity. Text / thinking deltas are intentionally
    /// dropped — the client sees only the sub-agent's tool work, not its
    /// incremental reasoning.
    ///
    /// The forwarding task is fire-and-forget and cleans up when the child
    /// bus is dropped (sender gone → receiver EOF → task exits).
    pub fn child_bus(&self, task_id: String) -> Self {
        let parent_tx = self.tx.clone();
        let (child_tx, mut child_rx) = mpsc::unbounded_channel::<LoopEvent>();

        tokio::spawn(async move {
            while let Some(event) = child_rx.recv().await {
                let forwarded = match event {
                    LoopEvent::ToolStart {
                        id,
                        name,
                        input_preview,
                    } => Some(LoopEvent::ToolStart {
                        id: format!("{task_id}:{id}"),
                        name,
                        input_preview,
                    }),
                    LoopEvent::ToolResult {
                        id,
                        name,
                        content,
                        is_error,
                    } => Some(LoopEvent::ToolResult {
                        id: format!("{task_id}:{id}"),
                        name,
                        content,
                        is_error,
                    }),
                    LoopEvent::HookBlocked { .. }
                    | LoopEvent::Compacted { .. }
                    | LoopEvent::Collapsed { .. } => Some(event),
                    // Text/thinking deltas stay local to the sub-agent.
                    _ => None,
                };
                if let Some(fwd) = forwarded {
                    let _ = parent_tx.send(fwd);
                }
            }
        });

        Self { tx: child_tx }
    }
}
