// ============================================================================
// SendMessage Use Case
//
// Orchestrates one user turn:
//   1. Persist user Message to DB
//   2. Create Run record (Pending)
//   3. Dispatch to Agent via gRPC
//   4. Transition Run to Running, persist
//   5. Spawn background task: read Redis Stream → accumulate → finalize
//
// The caller (gRPC handler) returns immediately with run_id.
// The background task handles the full agent lifecycle asynchronously.
//
// Completion notification:
//   When finalize_run() completes (message + run written to DB), it sends
//   CompletionData through the oneshot sender stored in CompletionRegistry.
//   The gRPC stream_events handler awaits that receiver instead of polling
//   the DB.
// ============================================================================

use std::sync::Arc;

use anyhow::{Context, Result};
use dashmap::DashMap;
use tokio::sync::oneshot;

use crate::domain::ids::RunId;
use crate::domain::{Message, Run, ThreadId};
use crate::infra::events::SessionEvent;
use crate::ports::agent_dispatcher::AgentDispatcher;
use crate::ports::event_stream::AgentEventStream;
use crate::ports::repository::{MessageRepo, RunRepo};

// ── Completion channel types ──────────────────────────────────────────────────

/// Data sent from the background run task to the gRPC stream handler when
/// `finalize_run` completes: the persisted assistant message ID and content.
#[derive(Debug, Clone)]
pub struct CompletionData {
    pub message_id: String,
    pub content: String,
}

/// Per-run registry of oneshot receivers.
///
/// Populated by `execute()` before the background task is spawned.
/// The `stream_events` gRPC handler removes the receiver and awaits it.
/// The background task holds the sender directly and calls `tx.send()` in
/// `finalize_run`. If the gRPC client disconnects, the receiver is dropped
/// and the send fails silently — the background task is unaffected.
pub type CompletionRegistry = Arc<DashMap<String, oneshot::Receiver<CompletionData>>>;

// ── Use case ──────────────────────────────────────────────────────────────────

/// Input to the SendMessage use case.
pub struct SendMessageInput {
    pub thread_id: ThreadId,
    /// The authenticated user submitting this message.
    pub user_id: String,
    pub content: String,
}

/// Immediate output: IDs for the client to poll or stream against.
///
/// The run itself continues asynchronously after `execute` returns.
pub struct SendMessageOutput {
    pub message_id: crate::domain::ids::MessageId,
    pub run_id: RunId,
}

/// Orchestrate one user turn end-to-end.
///
/// Returns quickly after dispatching to the agent. The full lifecycle
/// (stream → accumulate → finalize) runs in a background task.
pub async fn execute(
    input: SendMessageInput,
    run_repo: Arc<dyn RunRepo>,
    message_repo: Arc<dyn MessageRepo>,
    event_stream: Arc<dyn AgentEventStream>,
    agent_client: Arc<dyn AgentDispatcher>,
    completion_registry: CompletionRegistry,
) -> Result<SendMessageOutput> {
    // 1. Persist user message.
    let user_msg = Message::user(input.thread_id.clone(), &input.content);
    message_repo
        .insert(&user_msg)
        .await
        .context("failed to persist user message")?;

    // 2. Create Run in Pending state.
    let run = Run::new(input.thread_id.clone());
    let run_id = run.id.clone();
    run_repo
        .insert(&run)
        .await
        .context("failed to insert run")?;

    // 3. Dispatch to Agent. Agent assigns itself an ID and starts executing.
    let agent_id = agent_client
        .dispatch(
            input.thread_id.as_str(),
            run_id.as_str(),
            user_msg.id.as_str(),
            &input.user_id,
            &input.content,
        )
        .await
        .context("failed to dispatch to agent")?;

    // 4. Transition to Running and persist.
    let run = run
        .start(&agent_id)
        .context("invalid state transition to Running")?;
    run_repo
        .update(&run)
        .await
        .context("failed to update run to running")?;

    // 5. Create completion channel and register the Receiver before spawning.
    //    The stream_events gRPC handler removes and awaits the Receiver.
    //    The background task holds the Sender directly.
    let (completion_tx, completion_rx) = oneshot::channel::<CompletionData>();
    completion_registry.insert(run_id.to_string(), completion_rx);

    // 6. Spawn background task: stream agent events, accumulate, finalize.
    tokio::spawn(run_lifecycle(
        run,
        input.thread_id.as_str().to_string(),
        run_repo,
        message_repo,
        event_stream,
        completion_tx,
    ));

    Ok(SendMessageOutput {
        message_id: user_msg.id,
        run_id,
    })
}

// ── Background task ───────────────────────────────────────────────────────────

/// Background task that drives the run from Running → Completed (or Failed).
///
/// Reads Redis Stream events, accumulates text deltas, and finalizes
/// (writes assistant message + updates run) on RunCompleted.
async fn run_lifecycle(
    mut run: Run,
    thread_id: String,
    run_repo: Arc<dyn RunRepo>,
    message_repo: Arc<dyn MessageRepo>,
    event_stream: Arc<dyn AgentEventStream>,
    completion_tx: oneshot::Sender<CompletionData>,
) {
    let run_id = run.id.clone();

    let mut rx = match event_stream.subscribe(&run_id, "").await {
        Ok(rx) => rx,
        Err(e) => {
            tracing::error!(run_id = %run_id, error = %e, "failed to subscribe to agent events");
            // completion_tx dropped here — stream_events will time out gracefully.
            fail_run(run, "failed to subscribe to agent stream", &run_repo).await;
            return;
        }
    };

    // Accumulate events until the stream closes.
    while let Some(event) = rx.recv().await {
        match &event {
            SessionEvent::TextDelta { text } => {
                if let Err(e) = run.accumulate(text) {
                    tracing::warn!(run_id = %run_id, error = %e, "accumulate failed");
                }
            }
            SessionEvent::RunCompleted { .. } => {
                // Agent stream finished — transition and persist.
                let (run_completing, content) = match run.complete() {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::error!(run_id = %run_id, error = %e, "failed to complete run");
                        return;
                    }
                };
                run = run_completing;

                if let Err(e) = run_repo.update(&run).await {
                    tracing::error!(run_id = %run_id, error = %e, "failed to persist completing state");
                }

                finalize_run(run, content, &thread_id, run_repo, message_repo, completion_tx).await;
                return;
            }
            SessionEvent::RunFailed { reason } => {
                // completion_tx dropped here — stream_events will time out gracefully.
                fail_run(run, reason, &run_repo).await;
                return;
            }
            _ => {}
        }
    }

    // Stream ended without a terminal event — treat as interrupted.
    tracing::warn!(run_id = %run_id, "agent stream ended without terminal event");
    if let Ok(interrupted) = run.interrupt() {
        let _ = run_repo.update(&interrupted).await;
    }
}

/// Write assistant message to DB, finalize run, and notify the waiting gRPC stream.
async fn finalize_run(
    run: Run,
    content: String,
    thread_id: &str,
    run_repo: Arc<dyn RunRepo>,
    message_repo: Arc<dyn MessageRepo>,
    completion_tx: oneshot::Sender<CompletionData>,
) {
    let run_id = run.id.clone();
    let thread_id = ThreadId::from(thread_id);

    let assistant_msg = Message::assistant(thread_id, &content);
    let message_id = assistant_msg.id.clone();

    if let Err(e) = message_repo.insert(&assistant_msg).await {
        tracing::error!(run_id = %run_id, error = %e, "failed to persist assistant message");
        // completion_tx dropped — stream_events times out gracefully.
        return;
    }

    let run = match run.finalize(message_id.clone()) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(run_id = %run_id, error = %e, "failed to finalize run");
            return;
        }
    };

    if let Err(e) = run_repo.update(&run).await {
        tracing::error!(run_id = %run_id, error = %e, "failed to persist completed run");
    }

    // Notify the waiting gRPC stream with the persisted message data.
    let _ = completion_tx.send(CompletionData {
        message_id: message_id.to_string(),
        content,
    });
}

async fn fail_run(run: Run, reason: &str, run_repo: &Arc<dyn RunRepo>) {
    let run_id = run.id.clone();
    if let Ok(failed) = run.fail(reason) {
        let _ = run_repo.update(&failed).await;
    }
    tracing::error!(run_id = %run_id, reason = reason, "run failed");
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use anyhow::Result;
    use async_trait::async_trait;
    use dashmap::DashMap;
    use tokio::sync::Mutex;
    use tokio::sync::oneshot;

    use super::*;
    use crate::domain::{Message, MessageId, Run, RunId, RunStatus, Thread, ThreadId};
    use crate::ports::repository::{MessageRepo, RunRepo};

    #[derive(Default)]
    struct FakeRunRepo {
        runs: Mutex<HashMap<RunId, Run>>,
    }

    #[async_trait]
    impl RunRepo for FakeRunRepo {
        async fn insert(&self, run: &Run) -> Result<()> {
            self.runs.lock().await.insert(run.id.clone(), run.clone());
            Ok(())
        }
        async fn update(&self, run: &Run) -> Result<()> {
            self.runs.lock().await.insert(run.id.clone(), run.clone());
            Ok(())
        }
        async fn find_by_id(&self, id: &RunId) -> Result<Option<Run>> {
            Ok(self.runs.lock().await.get(id).cloned())
        }
        async fn find_latest_by_thread(&self, thread_id: &ThreadId) -> Result<Option<Run>> {
            let mut runs: Vec<_> = self
                .runs.lock().await.values()
                .filter(|r| &r.thread_id == thread_id)
                .cloned().collect();
            runs.sort_by_key(|r| r.created_at);
            Ok(runs.pop())
        }
        async fn find_by_status(&self, status: &RunStatus) -> Result<Vec<Run>> {
            Ok(self.runs.lock().await.values()
                .filter(|r| &r.status == status)
                .cloned().collect())
        }
    }

    #[derive(Default)]
    struct FakeMessageRepo {
        messages: Mutex<HashMap<MessageId, Message>>,
    }

    #[async_trait]
    impl MessageRepo for FakeMessageRepo {
        async fn insert(&self, message: &Message) -> Result<()> {
            self.messages.lock().await.insert(message.id.clone(), message.clone());
            Ok(())
        }
        async fn list_by_thread(&self, thread_id: &ThreadId, limit: i64, offset: i64) -> Result<Vec<Message>> {
            let mut msgs: Vec<_> = self.messages.lock().await.values()
                .filter(|m| &m.thread_id == thread_id)
                .cloned().collect();
            msgs.sort_by_key(|m| m.created_at);
            Ok(msgs.into_iter().skip(offset.max(0) as usize).take(limit.max(0) as usize).collect())
        }
    }

    /// The completion channel correctly delivers finalized run data
    /// from the background task to the gRPC stream handler.
    #[tokio::test]
    async fn completion_channel_delivers_finalized_data() {
        let registry: CompletionRegistry = Arc::new(DashMap::new());
        let thread = Thread::new("user-1", "Thread");
        let run = Run::new(thread.id.clone()).start("agent-1").unwrap();
        let run_id_str = run.id.to_string();

        // Simulate execute(): store receiver, pass sender to background task.
        let (tx, rx) = oneshot::channel::<CompletionData>();
        registry.insert(run_id_str.clone(), rx);

        // Simulate the background task completing.
        let (run_completing, _) = run.complete().unwrap();
        let assistant_msg = Message::assistant(thread.id.clone(), "hello from agent");
        let message_id = assistant_msg.id.clone();
        let run_done = run_completing.finalize(message_id.clone()).unwrap();
        let _ = run_done;

        tx.send(CompletionData {
            message_id: message_id.to_string(),
            content: "hello from agent".to_string(),
        }).unwrap();

        // Simulate stream_events receiving.
        let (_, stored_rx) = registry.remove(&run_id_str).unwrap();
        let data = stored_rx.await.unwrap();

        assert_eq!(data.message_id, message_id.to_string());
        assert_eq!(data.content, "hello from agent");
    }
}
