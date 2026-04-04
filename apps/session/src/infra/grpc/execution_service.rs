// ============================================================================
// AgentExecutionService — Run Lifecycle + Streaming Events
//
// Handles the execution side of the session gRPC API:
//   - SendMessage:   persists user message, creates run, dispatches to agent
//   - StreamEvents:  server-streaming RPC, forwards agent events to client
//   - ResumeRun:     forwards a control response to the agent
//   - CancelRun:     cancels the active agent run
//   - GetRunStatus:  returns the current run state
// ============================================================================

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_stream::stream;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};

use hahi_proto::chat::{
    CancelRunRequest, CancelRunResponse, GetRunStatusRequest, GetRunStatusResponse,
    ResumeRunRequest, ResumeRunResponse, SendMessageRequest,
    SendMessageResponse, StreamEventsRequest, agent_execution_service_server::AgentExecutionService,
};
use hahi_proto::events::EventFrame;

use crate::app::send_message::{self, CompletionRegistry, SendMessageInput};
use crate::app::stream_events::{self, StreamEventsInput};
use crate::domain::{RunStatus, ThreadId};
use crate::infra::events::SessionEvent;
use crate::infra::grpc::event_projection::{hub_event_to_frame, hub_run_completed_to_frame};
use crate::ports::agent_dispatcher::AgentDispatcher;
use crate::ports::event_stream::AgentEventStream;
use crate::ports::repository::{MessageRepo, RunRepo};

/// How long to wait for `finalize_run` to deliver completion data before
/// falling back to empty fields. 10 seconds covers very slow DB writes.
const COMPLETION_WAIT_TIMEOUT: Duration = Duration::from_secs(10);

/// State for the `AgentExecutionService` gRPC implementation.
///
/// Cloned cheaply per request — all fields are `Arc`.
#[derive(Clone)]
pub struct ExecutionServiceImpl {
    pub run_repo: Arc<dyn RunRepo>,
    pub message_repo: Arc<dyn MessageRepo>,
    pub event_stream: Arc<dyn AgentEventStream>,
    pub agent_dispatcher: Arc<dyn AgentDispatcher>,
    /// Completion channel registry — connects `finalize_run` (background task)
    /// to `stream_events` (gRPC stream) without polling.
    pub completion_registry: CompletionRegistry,
}

type StreamEventsResult = Pin<Box<dyn Stream<Item = Result<EventFrame, Status>> + Send>>;

#[tonic::async_trait]
impl AgentExecutionService for ExecutionServiceImpl {
    type StreamEventsStream = StreamEventsResult;

    async fn send_message(
        &self,
        request: Request<SendMessageRequest>,
    ) -> Result<Response<SendMessageResponse>, Status> {
        let req = request.into_inner();

        let output = send_message::execute(
            SendMessageInput {
                thread_id: ThreadId::from(req.thread_id),
                user_id: req.user_id,
                content: req.content,
            },
            Arc::clone(&self.run_repo),
            Arc::clone(&self.message_repo),
            Arc::clone(&self.event_stream),
            Arc::clone(&self.agent_dispatcher),
            Arc::clone(&self.completion_registry),
        )
        .await
        .map_err(|e| Status::internal(format!("send_message failed: {e}")))?;

        Ok(Response::new(SendMessageResponse {
            message_id: output.message_id.to_string(),
            run_id: output.run_id.to_string(),
        }))
    }

    async fn stream_events(
        &self,
        request: Request<StreamEventsRequest>,
    ) -> Result<Response<Self::StreamEventsStream>, Status> {
        let req = request.into_inner();
        let run_id = crate::domain::ids::RunId::from(req.run_id.clone());

        let run = self
            .run_repo
            .find_by_id(&run_id)
            .await
            .map_err(|e| Status::internal(format!("failed to look up run {}: {e}", req.run_id)))?
            .ok_or_else(|| Status::not_found(format!("run {} not found", req.run_id)))?;

        let mut receiver = stream_events::subscribe(
            StreamEventsInput {
                run_id: run_id.clone(),
                last_event_id: req.last_event_id,
            },
            Arc::clone(&self.event_stream),
        )
        .await
        .map_err(|e| Status::internal(format!("failed to subscribe to run {}: {e}", run.id)))?;

        let run_id_str = req.run_id;
        let thread_id_str = run.thread_id.to_string();
        let completion_registry = Arc::clone(&self.completion_registry);

        let event_stream = stream! {
            let mut seq: u64 = 0;
            while let Some(event) = receiver.recv().await {
                seq += 1;
                let event_id = seq.to_string();

                let frame = match &event {
                    SessionEvent::RunCompleted { input_tokens, output_tokens } => {
                        let completion = if let Some((_, rx)) = completion_registry.remove(&run_id_str) {
                            match tokio::time::timeout(COMPLETION_WAIT_TIMEOUT, rx).await {
                                Ok(Ok(data)) => Some((data.message_id, data.content)),
                                Ok(Err(_)) => {
                                    tracing::error!(run_id = %run_id_str, "completion sender dropped without data");
                                    None
                                }
                                Err(_) => {
                                    tracing::error!(run_id = %run_id_str, "timed out waiting for run completion data");
                                    None
                                }
                            }
                        } else {
                            // Registry entry absent — stream connected after finalization or session
                            // restarted. The run is already completed in DB; client should re-fetch.
                            tracing::warn!(run_id = %run_id_str, "no completion receiver in registry");
                            None
                        };

                        match completion {
                            Some((message_id, content)) => hub_run_completed_to_frame(
                                event_id,
                                run_id_str.clone(),
                                thread_id_str.clone(),
                                message_id,
                                content,
                                *input_tokens,
                                *output_tokens,
                            ),
                            None => hub_event_to_frame(
                                event_id,
                                run_id_str.clone(),
                                thread_id_str.clone(),
                                &SessionEvent::RunFailed {
                                    reason: "run finalization unavailable".to_string(),
                                },
                            ),
                        }
                    }
                    _ => hub_event_to_frame(
                        event_id,
                        run_id_str.clone(),
                        thread_id_str.clone(),
                        &event,
                    ),
                };

                yield Ok(frame);
            }
        };

        Ok(Response::new(Box::pin(event_stream)))
    }

    async fn resume_run(
        &self,
        request: Request<ResumeRunRequest>,
    ) -> Result<Response<ResumeRunResponse>, Status> {
        let req = request.into_inner();
        let control = req
            .control
            .ok_or_else(|| Status::invalid_argument("control response is required"))?;
        let thread_id = ThreadId::from(req.thread_id.clone());
        let run_id = if req.run_id.is_empty() {
            self.run_repo
                .find_latest_by_thread(&thread_id)
                .await
                .map_err(|e| Status::internal(e.to_string()))?
                .map(|run| run.id.to_string())
                .ok_or_else(|| Status::failed_precondition("no run found for thread"))?
        } else {
            req.run_id.clone()
        };

        let resumed_run_id = self
            .agent_dispatcher
            .resume_run(&req.thread_id, &run_id, &req.user_id, control)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(ResumeRunResponse {
            run_id: resumed_run_id,
        }))
    }

    async fn cancel_run(
        &self,
        request: Request<CancelRunRequest>,
    ) -> Result<Response<CancelRunResponse>, Status> {
        let req = request.into_inner();
        let cancelled = self
            .agent_dispatcher
            .cancel_run(&req.thread_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(CancelRunResponse { cancelled }))
    }

    async fn get_run_status(
        &self,
        request: Request<GetRunStatusRequest>,
    ) -> Result<Response<GetRunStatusResponse>, Status> {
        let req = request.into_inner();
        let run_id = crate::domain::ids::RunId::from(req.run_id);
        let run = self
            .run_repo
            .find_by_id(&run_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("run not found"))?;

        if matches!(
            &run.status,
            RunStatus::Completed | RunStatus::Failed | RunStatus::Interrupted
        ) {
            return Ok(Response::new(GetRunStatusResponse {
                status: run.status.to_string(),
                active_request_id: None,
            }));
        }

        if let Ok((agent_status, active_request_id)) = self
            .agent_dispatcher
            .get_run_status(run.thread_id.as_str())
            .await
        {
            if agent_status != "idle" {
                return Ok(Response::new(GetRunStatusResponse {
                    status: run.status.to_string(),
                    active_request_id,
                }));
            }
        }

        Ok(Response::new(GetRunStatusResponse {
            status: run.status.to_string(),
            active_request_id: None,
        }))
    }
}

