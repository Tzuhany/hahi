// ============================================================================
// gRPC Agent Execution Service
//
// Thin adapter layer for the agent execution API. Conversation ownership lives
// outside this service; this module only exposes execution lifecycle RPCs.
// ============================================================================

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tonic::{Request, Response, Status};

use hahi_proto::chat::agent_execution_service_server::{
    AgentExecutionService, AgentExecutionServiceServer,
};
use hahi_proto::chat::{
    CancelRunRequest, CancelRunResponse, GetRunStatusRequest, GetRunStatusResponse,
    ResumeRunRequest, ResumeRunResponse, SendMessageRequest, SendMessageResponse,
    StreamEventsRequest,
};
use hahi_proto::events::EventFrame;

use crate::adapters::llm::providers::create_provider;
use crate::adapters::mcp::client::{McpClient, McpServerConfig};
use crate::adapters::mcp::registry::McpToolAdapter;
use crate::adapters::metrics::Metrics;
use crate::adapters::store::Store;
use crate::common::Checkpoint;
use crate::config::Config;
use crate::kernel::control::resume_message;
use crate::runtime::assembler::{RunPipeline, RunRequest};
use crate::runtime::prompt::cache::PromptCache;
use crate::runtime::state::{RunRegistry, RunStatus};
use crate::systems::memory::MemoryEngine;
use crate::systems::memory::embed::NoOpEmbedder;
use crate::systems::skills::loader::SkillLoader;
use crate::systems::tools::definition::Tool;

/// Shared state cloned into every gRPC handler call by tonic.
#[derive(Clone)]
pub struct AgentState {
    pub pipeline: Arc<RunPipeline>,
    runs: RunRegistry,
}

#[async_trait]
impl AgentExecutionService for AgentState {
    type StreamEventsStream =
        std::pin::Pin<Box<dyn futures::Stream<Item = Result<EventFrame, tonic::Status>> + Send>>;

    async fn send_message(
        &self,
        request: Request<SendMessageRequest>,
    ) -> Result<Response<SendMessageResponse>, Status> {
        let req = request.into_inner();
        if req.run_id.is_empty() {
            return Err(Status::invalid_argument("run_id is required"));
        }
        let message_id = uuid::Uuid::new_v4().to_string();
        let cancel = CancellationToken::new();
        let stream_run_id = req.run_id.clone();

        self.runs.mark_running(&req.thread_id, cancel.clone()).await;

        let output = self
            .pipeline
            .execute_with_cancel(
                RunRequest {
                    thread_id: &req.thread_id,
                    run_id: &stream_run_id,
                    user_id: &req.user_id,
                    content: &req.content,
                    message_id: &message_id,
                },
                cancel,
            )
            .await;

        match output {
            Ok(output) => {
                self.runs.apply_output(&req.thread_id, &output).await;
                Ok(Response::new(SendMessageResponse {
                    message_id: output.message_id,
                    run_id: stream_run_id,
                }))
            }
            Err(e) => {
                self.runs.mark_idle(&req.thread_id).await;
                Err(Status::internal(e.to_string()))
            }
        }
    }

    async fn resume_run(
        &self,
        request: Request<ResumeRunRequest>,
    ) -> Result<Response<ResumeRunResponse>, Status> {
        let req = request.into_inner();
        let control = req
            .control
            .ok_or_else(|| Status::invalid_argument("control response is required"))?;
        let checkpoint = self
            .load_checkpoint(&req.thread_id)
            .await?
            .ok_or_else(|| Status::failed_precondition("no checkpoint found for resume"))?;
        let pending = checkpoint.pending_control.ok_or_else(|| {
            Status::failed_precondition("conversation is not waiting on a control response")
        })?;

        if pending.request_id != control.request_id {
            return Err(Status::failed_precondition(format!(
                "request_id mismatch: expected '{}', got '{}'",
                pending.request_id, control.request_id
            )));
        }

        if req.run_id.is_empty() {
            return Err(Status::invalid_argument("run_id is required"));
        }
        let content = resume_message(&pending, &control).map_err(Status::invalid_argument)?;
        let message_id = uuid::Uuid::new_v4().to_string();
        let stream_run_id = req.run_id.clone();
        let cancel = CancellationToken::new();
        self.runs.mark_running(&req.thread_id, cancel.clone()).await;

        let output = self
            .pipeline
            .execute_with_cancel(
                RunRequest {
                    thread_id: &req.thread_id,
                    run_id: &stream_run_id,
                    user_id: &req.user_id,
                    content: &content,
                    message_id: &message_id,
                },
                cancel,
            )
            .await;

        match output {
            Ok(output) => {
                self.runs.apply_output(&req.thread_id, &output).await;
                Ok(Response::new(ResumeRunResponse {
                    run_id: stream_run_id,
                }))
            }
            Err(e) => {
                self.runs.mark_idle(&req.thread_id).await;
                Err(Status::internal(e.to_string()))
            }
        }
    }

    async fn cancel_run(
        &self,
        request: Request<CancelRunRequest>,
    ) -> Result<Response<CancelRunResponse>, Status> {
        let thread_id = request.into_inner().thread_id;
        let cancelled = self.runs.cancel(&thread_id).await;
        Ok(Response::new(CancelRunResponse { cancelled }))
    }

    async fn get_run_status(
        &self,
        request: Request<GetRunStatusRequest>,
    ) -> Result<Response<GetRunStatusResponse>, Status> {
        let run_id = request.into_inner().run_id;

        if let Some(state) = self.runs.get(&run_id).await {
            return Ok(Response::new(GetRunStatusResponse {
                status: state.status.as_proto().to_string(),
                active_request_id: state.active_request_id,
            }));
        }

        let checkpoint = self.load_checkpoint(&run_id).await?;
        let (run_status, active_request_id) = checkpoint
            .and_then(|cp| {
                cp.pending_control
                    .map(|pending| (RunStatus::RequiresAction, Some(pending.request_id)))
            })
            .unwrap_or_else(|| (RunStatus::Idle, None));

        Ok(Response::new(GetRunStatusResponse {
            status: run_status.as_proto().to_string(),
            active_request_id,
        }))
    }

    async fn stream_events(
        &self,
        request: Request<StreamEventsRequest>,
    ) -> Result<Response<Self::StreamEventsStream>, Status> {
        // The agent does not implement stream_events — the session service owns that RPC.
        // This stub satisfies the trait bound; the session service never calls it on the agent.
        let _ = request;
        Err(Status::unimplemented(
            "stream_events is served by the session service",
        ))
    }
}

impl AgentState {
    async fn load_checkpoint(&self, thread_id: &str) -> Result<Option<Checkpoint>, Status> {
        if let Some(bytes) = self
            .pipeline
            .store
            .redis_load_checkpoint(thread_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
        {
            let checkpoint =
                serde_json::from_slice(&bytes).map_err(|e| Status::internal(e.to_string()))?;
            return Ok(Some(checkpoint));
        }
        if let Some(bytes) = self
            .pipeline
            .store
            .pg_load_checkpoint(thread_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
        {
            let checkpoint =
                serde_json::from_slice(&bytes).map_err(|e| Status::internal(e.to_string()))?;
            return Ok(Some(checkpoint));
        }
        Ok(None)
    }
}

/// Start the gRPC server and a Prometheus `/metrics` HTTP server.
pub async fn serve(config: Config) -> anyhow::Result<()> {
    let store = Arc::new(Store::connect(&config.database_url, &config.redis_url).await?);

    let provider = create_provider(config.llm_provider.clone())?;
    let memory = MemoryEngine::new(Arc::clone(&store), Arc::new(NoOpEmbedder));
    let skill_loader = Arc::new(SkillLoader::new(config.skills_dir));
    let metrics = Arc::new(Metrics::new());
    let prompt_cache = Arc::new(Mutex::new(PromptCache::new()));
    let mcp_tools = load_mcp_tools(&config.mcp_servers).await?;

    let pipeline = Arc::new(RunPipeline {
        store: Arc::clone(&store),
        provider,
        memory,
        skill_loader,
        metrics: Arc::clone(&metrics),
        prompt_cache,
        mcp_tools,
    });

    let state = AgentState {
        pipeline,
        runs: RunRegistry::default(),
    };

    let grpc_addr: std::net::SocketAddr = config
        .grpc_addr
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid gRPC address '{}': {}", config.grpc_addr, e))?;
    let metrics_addr: std::net::SocketAddr = config
        .metrics_addr
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid metrics address '{}': {}", config.metrics_addr, e))?;

    tracing::info!(grpc = %grpc_addr, metrics = %metrics_addr, "agent starting");

    let metrics_listener = tokio::net::TcpListener::bind(metrics_addr)
        .await
        .map_err(|e| anyhow::anyhow!("failed to bind metrics address '{}': {}", metrics_addr, e))?;

    let metrics_server = {
        let m = Arc::clone(&metrics);
        let listener = metrics_listener;
        tokio::spawn(async move {
            use axum::{Router, routing::get};
            let app = Router::new().route(
                "/metrics",
                get(move || {
                    let m = Arc::clone(&m);
                    async move { m.to_prometheus() }
                }),
            );
            tracing::info!(addr = %metrics_addr, "metrics server listening");
            if let Err(e) = axum::serve(listener, app).await {
                tracing::error!(addr = %metrics_addr, error = %e, "metrics server error");
            }
        })
    };

    let grpc_result = tonic::transport::Server::builder()
        .add_service(AgentExecutionServiceServer::new(state))
        .serve(grpc_addr)
        .await
        .map_err(|e| anyhow::anyhow!("gRPC server error: {e}"));

    metrics_server.abort();
    tracing::info!("agent shutting down");
    grpc_result
}

async fn load_mcp_tools(configs: &[McpServerConfig]) -> Result<Vec<Arc<dyn Tool>>> {
    let mut tools: Vec<Arc<dyn Tool>> = Vec::new();

    for server in configs {
        let mut client = McpClient::new(server.clone());
        let defs = client.connect().await?;
        let client = Arc::new(tokio::sync::Mutex::new(client));

        for def in defs {
            tools.push(Arc::new(McpToolAdapter::new(
                &server.name,
                &def.name,
                &def.description,
                def.input_schema,
                server.always_load,
                Arc::clone(&client),
            )));
        }
    }

    if !tools.is_empty() {
        tracing::info!(mcp_tools = tools.len(), "registered MCP tools");
    }

    Ok(tools)
}
