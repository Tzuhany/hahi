// ============================================================================
// Hahi Agent
//
// gRPC service that executes AI agent loops.
//
// Foundation:
//   common/ — domain types (Message, ContentBlock, TokenUsage, Checkpoint, etc.)
//   infra/  — infrastructure (store/pg, store/redis, metrics)
//
// Compute:
//   core/   — loop, compression (L1/L2/L3), hooks, permissions, plan mode
//   llm/    — multi-provider LLM client (Anthropic, OpenAI)
//   tool/   — two-tier registry, schema validation, streaming executor
//   skill/  — filesystem skill loading + budget-controlled listing
//   multi/  — sub-agent spawning with depth tracking + fork cache
//   mcp/    — Model Context Protocol client for external tools
//   prompt/ — system prompt assembly with cache boundary + section caching
//   memory/ — four-type memory system with hybrid recall
// ============================================================================

mod common;
mod config;
mod core;
mod infra;
mod llm;
mod mcp;
mod memory;
mod multi;
mod prompt;
mod run;
mod service;
mod skill;
mod tool;

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

    service::serve(config).await
}
