// ============================================================================
// gRPC Agent Service
//
// Thin adapter layer: translates proto types ↔ domain types and delegates
// all execution logic to RunPipeline (see run.rs).
//
// Each handler:
//   1. Extracts fields from the proto request
//   2. Calls RunPipeline::execute (or a Store helper)
//   3. Maps the result into a proto response
//
// No business logic lives here.
// ============================================================================

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

use hahi_proto::chat::chat_service_server::{ChatService, ChatServiceServer};
use hahi_proto::chat::{
    DeleteConversationRequest, DeleteConversationResponse, GetConversationRequest,
    GetConversationResponse, ListConversationsRequest, ListConversationsResponse,
    ListMessagesRequest, ListMessagesResponse, SendMessageRequest, SendMessageResponse,
};

use crate::config::Config;
use crate::infra::metrics::Metrics;
use crate::infra::store::Store;
use crate::llm::providers::{ProviderKind, create_provider};
use crate::mcp::client::{McpClient, McpServerConfig};
use crate::mcp::registry::McpToolAdapter;
use crate::memory::MemoryEngine;
use crate::memory::embed::NoOpEmbedder;
use crate::prompt::cache::PromptCache;
use crate::run::{RunPipeline, RunRequest};
use crate::skill::SkillLoader;
use crate::tool::definition::Tool;

// ============================================================================
// Shared State
// ============================================================================

/// Shared state cloned into every gRPC handler call by tonic.
/// All fields are Arc-wrapped so clone is cheap (pointer copy).
#[derive(Clone)]
pub struct AgentState {
    pub pipeline: Arc<RunPipeline>,
}

// ============================================================================
// ChatService Implementation
// ============================================================================

#[async_trait]
impl ChatService for AgentState {
    // ── SendMessage ──────────────────────────────────────────────────────────

    async fn send_message(
        &self,
        request: Request<SendMessageRequest>,
    ) -> Result<Response<SendMessageResponse>, Status> {
        let req = request.into_inner();
        let message_id = uuid::Uuid::new_v4().to_string();

        let output = self
            .pipeline
            .execute(RunRequest {
                thread_id: &req.conversation_id,
                user_id: &req.user_id,
                content: &req.content,
                message_id: &message_id,
            })
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(SendMessageResponse {
            message_id: output.message_id,
            stream_key: output.stream_key,
        }))
    }

    // ── GetConversation ──────────────────────────────────────────────────────

    async fn get_conversation(
        &self,
        _request: Request<GetConversationRequest>,
    ) -> Result<Response<GetConversationResponse>, Status> {
        Err(Status::unimplemented(
            "conversation metadata is owned by the Conversation module",
        ))
    }

    // ── ListConversations ────────────────────────────────────────────────────

    async fn list_conversations(
        &self,
        _request: Request<ListConversationsRequest>,
    ) -> Result<Response<ListConversationsResponse>, Status> {
        Err(Status::unimplemented(
            "conversation listing is owned by the Conversation module",
        ))
    }

    // ── DeleteConversation ───────────────────────────────────────────────────

    async fn delete_conversation(
        &self,
        _request: Request<DeleteConversationRequest>,
    ) -> Result<Response<DeleteConversationResponse>, Status> {
        Err(Status::unimplemented(
            "conversation deletion is owned by the Conversation module",
        ))
    }

    // ── ListMessages ─────────────────────────────────────────────────────────

    async fn list_messages(
        &self,
        _request: Request<ListMessagesRequest>,
    ) -> Result<Response<ListMessagesResponse>, Status> {
        Err(Status::unimplemented(
            "message history is owned by the Conversation module",
        ))
    }
}

// ============================================================================
// Server Startup
// ============================================================================

/// Start the gRPC server and a Prometheus `/metrics` HTTP server.
pub async fn serve(config: Config) -> anyhow::Result<()> {
    let store = Arc::new(Store::connect(&config.database_url, &config.redis_url).await?);

    let provider = create_provider(ProviderKind::Anthropic)?;
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

    let state = AgentState { pipeline };

    let grpc_addr: std::net::SocketAddr = config
        .grpc_addr
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid gRPC address '{}': {}", config.grpc_addr, e))?;
    let metrics_addr: std::net::SocketAddr = config
        .metrics_addr
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid metrics address '{}': {}", config.metrics_addr, e))?;

    tracing::info!(grpc = %grpc_addr, metrics = %metrics_addr, "agent starting");

    let metrics_server = {
        let m = Arc::clone(&metrics);
        tokio::spawn(async move {
            use axum::{Router, routing::get};
            let app = Router::new().route(
                "/metrics",
                get(move || {
                    let m = Arc::clone(&m);
                    async move { m.to_prometheus() }
                }),
            );
            let listener = tokio::net::TcpListener::bind(metrics_addr)
                .await
                .expect("failed to bind metrics address");
            tracing::info!(addr = %metrics_addr, "metrics server listening");
            axum::serve(listener, app)
                .await
                .expect("metrics server error");
        })
    };

    let grpc_result = tonic::transport::Server::builder()
        .add_service(ChatServiceServer::new(state))
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
