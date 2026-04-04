// ============================================================================
// Session Service — Entry Point
//
// Startup sequence:
//   1. Load config from environment
//   2. Connect to PostgreSQL and run pending migrations
//   3. Connect to Redis
//   4. Build infrastructure adapters (repos, event stream, agent client)
//   5. Crash recovery: interrupt orphaned runs from previous process lifecycle
//   6. Start gRPC server
// ============================================================================

mod app;
mod config;
mod domain;
mod infra;
mod ports;

use std::sync::Arc;

use anyhow::Result;
use dashmap::DashMap;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use hahi_proto::chat::{
    agent_execution_service_server::AgentExecutionServiceServer,
    thread_service_server::ThreadServiceServer,
};

use crate::app::send_message::CompletionRegistry;
use crate::config::Config;
use crate::domain::RunStatus;
use crate::infra::grpc::agent_client::AgentClient;
use crate::infra::grpc::execution_service::ExecutionServiceImpl;
use crate::infra::grpc::thread_service::ThreadServiceImpl;
use crate::infra::pg::{
    message_repo::PgMessageRepo, run_repo::PgRunRepo, thread_repo::PgThreadRepo,
};
use crate::infra::redis::event_stream::RedisEventStream;
use crate::ports::agent_dispatcher::AgentDispatcher;
use crate::ports::repository::RunRepo;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::from_env()?;

    // ── Database ──────────────────────────────────────────────────────────────

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(20)
        .connect(&config.database_url)
        .await?;

    sqlx::migrate!("../../db/migrations/session")
        .run(&pool)
        .await?;

    // ── Redis ─────────────────────────────────────────────────────────────────

    let redis_client = redis::Client::open(config.redis_url.as_str())?;

    // ── Infrastructure adapters ───────────────────────────────────────────────

    let thread_repo = Arc::new(PgThreadRepo::new(pool.clone()));
    let run_repo = Arc::new(PgRunRepo::new(pool.clone()));
    let message_repo = Arc::new(PgMessageRepo::new(pool.clone()));
    let event_stream = Arc::new(RedisEventStream::new(redis_client));
    let agent_dispatcher: Arc<dyn AgentDispatcher> =
        Arc::new(AgentClient::connect(&config.agent_url).await?);

    // ── Crash recovery ────────────────────────────────────────────────────────
    // Runs left in Running or Completing from a previous process lifecycle are
    // unreachable — their background tasks died with the process. Mark them
    // Interrupted so clients can surface an error and users can retry.
    for status in [RunStatus::Running, RunStatus::Completing] {
        match run_repo.find_by_status(&status).await {
            Ok(orphans) => {
                for run in orphans {
                    let run_id = run.id.clone();
                    match run.interrupt() {
                        Ok(interrupted) => {
                            if let Err(e) = run_repo.update(&interrupted).await {
                                tracing::error!(run_id = %run_id, error = %e, "failed to interrupt orphaned run");
                            } else {
                                tracing::warn!(run_id = %run_id, "interrupted orphaned run on startup");
                            }
                        }
                        Err(e) => {
                            tracing::error!(run_id = %run_id, error = %e, "unexpected state for orphaned run");
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!(status = %status, error = %e, "failed to query orphaned runs on startup");
            }
        }
    }

    // ── gRPC services ─────────────────────────────────────────────────────────

    let completion_registry: CompletionRegistry = Arc::new(DashMap::new());

    let run_repo_dyn: Arc<dyn RunRepo> = run_repo;
    let message_repo_dyn: Arc<dyn crate::ports::repository::MessageRepo> = message_repo;
    let thread_repo_dyn: Arc<dyn crate::ports::repository::ThreadRepo> = thread_repo;

    let execution_service = ExecutionServiceImpl {
        run_repo: Arc::clone(&run_repo_dyn),
        message_repo: Arc::clone(&message_repo_dyn),
        event_stream,
        agent_dispatcher,
        completion_registry,
    };

    let thread_service = ThreadServiceImpl {
        thread_repo: thread_repo_dyn,
        message_repo: message_repo_dyn,
    };

    let addr: std::net::SocketAddr = config.grpc_addr.parse()?;
    tracing::info!(addr = %addr, "session-service listening");

    tonic::transport::Server::builder()
        .add_service(AgentExecutionServiceServer::new(execution_service))
        .add_service(ThreadServiceServer::new(thread_service))
        .serve(addr)
        .await?;

    Ok(())
}
