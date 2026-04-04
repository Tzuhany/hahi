// ============================================================================
// Configuration
//
// Loaded once at startup from environment variables.
// Hard errors on missing required values — fail fast, no silent defaults.
// ============================================================================

use anyhow::{Context, Result};

pub struct Config {
    /// PostgreSQL connection string.
    pub database_url: String,
    /// Redis connection string.
    pub redis_url: String,
    /// Address to bind the gRPC server on, e.g. "0.0.0.0:50051".
    pub grpc_addr: String,
    /// gRPC address of the Agent service, e.g. "http://agent:50052".
    pub agent_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            database_url: std::env::var("DATABASE_URL").context("DATABASE_URL is required")?,
            redis_url: std::env::var("REDIS_URL").context("REDIS_URL is required")?,
            grpc_addr: std::env::var("GRPC_ADDR").unwrap_or_else(|_| "0.0.0.0:50051".into()),
            agent_url: std::env::var("AGENT_URL").context("AGENT_URL is required")?,
        })
    }
}
