// ============================================================================
// Gateway Configuration
//
// Gateway is stateless — it holds only gRPC channel handles and JWT config.
// No database, no Redis. All state lives in the session service.
// ============================================================================

use anyhow::{Context, Result};
use tonic::transport::Channel;

use crate::clients::SessionClient;

#[derive(Clone)]
pub struct AppState {
    pub session: SessionClient,
    pub jwt_secret: String,
}

pub struct Config {
    pub port: u16,
    pub session_url: String,
    pub jwt_secret: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            port: std::env::var("GATEWAY_PORT")
                .unwrap_or_else(|_| "3000".into())
                .parse()
                .context("GATEWAY_PORT must be a valid port number")?,
            session_url: std::env::var("SESSION_URL").context("SESSION_URL is required")?,
            jwt_secret: std::env::var("JWT_SECRET").context("JWT_SECRET is required")?,
        })
    }

    pub async fn build_state(&self) -> Result<AppState> {
        let channel = Channel::from_shared(self.session_url.clone())
            .context("invalid SESSION_URL")?
            .connect()
            .await
            .with_context(|| {
                format!(
                    "failed to connect to session service at {}",
                    self.session_url
                )
            })?;

        Ok(AppState {
            session: SessionClient::new(channel),
            jwt_secret: self.jwt_secret.clone(),
        })
    }
}
