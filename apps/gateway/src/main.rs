mod clients;
mod config;
mod error;
mod handlers;
mod middleware;
mod router;

// SSE fan-out kept for future multi-instance fan-out work.
#[allow(dead_code)]
mod sse;

use anyhow::Result;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = config::Config::from_env()?;
    let state = config.build_state().await?;
    let router = router::build(state);

    let addr = format!("0.0.0.0:{}", config.port);
    tracing::info!(addr = %addr, "gateway listening");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}
