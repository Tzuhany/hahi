// ============================================================================
// Hahi Agent
//
// gRPC service that executes AI agent loops.
//
// Macro structure:
//   common/   — zero-dependency domain vocabulary
//   kernel/   — execution kernel: loop, control flow, compression, permissions
//   runtime/  — turn assembly, runtime state, prompt construction, reflection
//   systems/  — agent subsystems: memory, tools, skills, sub-agents
//   adapters/ — external interfaces: gRPC, LLMs, MCP, store, metrics
// ============================================================================

mod adapters;
mod common;
mod config;
#[cfg(test)]
mod eval;
mod kernel;
mod runtime;
mod systems;

use anyhow::Result;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = config::Config::from_env()?;
    tracing::info!(grpc_addr = config.grpc_addr, "hahi agent starting");

    adapters::grpc::serve(config).await
}
