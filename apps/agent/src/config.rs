// ============================================================================
// Agent Configuration
//
// Validated eagerly at startup — fail fast with clear error messages.
// ============================================================================

use anyhow::{Context, Result};

use crate::adapters::llm::providers::ProviderKind;
use crate::adapters::mcp::client::McpServerConfig;

pub struct Config {
    /// PostgreSQL URL (threads, runs, messages, memories).
    pub database_url: String,

    /// Redis URL (event streaming, checkpoints).
    pub redis_url: String,

    /// gRPC listen address for this agent service.
    pub grpc_addr: String,

    /// Which LLM provider implementation to use.
    pub llm_provider: ProviderKind,

    /// Path to skills directory on disk.
    pub skills_dir: String,

    /// Address for the Prometheus /metrics HTTP server.
    pub metrics_addr: String,

    /// Optional MCP servers to connect at startup.
    pub mcp_servers: Vec<McpServerConfig>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let config = Self {
            database_url: required_env("DATABASE_URL")?,
            redis_url: required_env("REDIS_URL")?,
            grpc_addr: std::env::var("AGENT_GRPC_ADDR").unwrap_or_else(|_| "0.0.0.0:50060".into()),
            llm_provider: optional_env("LLM_PROVIDER")
                .unwrap_or_else(|| "anthropic".into())
                .parse()?,
            skills_dir: std::env::var("SKILLS_DIR").unwrap_or_else(|_| "data/skills".into()),
            metrics_addr: std::env::var("METRICS_ADDR").unwrap_or_else(|_| "0.0.0.0:9090".into()),
            mcp_servers: optional_json_env("MCP_SERVERS_JSON")?,
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.database_url.starts_with("postgres://")
                || self.database_url.starts_with("postgresql://"),
            "DATABASE_URL must start with postgres://"
        );
        anyhow::ensure!(
            self.redis_url.starts_with("redis://") || self.redis_url.starts_with("rediss://"),
            "REDIS_URL must start with redis://"
        );
        Ok(())
    }
}

fn required_env(name: &str) -> Result<String> {
    std::env::var(name).context(format!("{name} is required"))
}

fn optional_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn optional_json_env<T>(name: &str) -> Result<T>
where
    T: serde::de::DeserializeOwned + Default,
{
    match std::env::var(name) {
        Ok(raw) => serde_json::from_str(&raw).context(format!("{name} must be valid JSON")),
        Err(std::env::VarError::NotPresent) => Ok(T::default()),
        Err(e) => Err(anyhow::anyhow!("{name} could not be read: {e}")),
    }
}
