use anyhow::Result;

#[derive(Clone)]
pub struct AppState {
    pub redis: redis::Client,
    // gRPC clients to services are created per-request or pooled here
}

pub struct Config {
    pub port: u16,
    pub redis_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            port: std::env::var("GATEWAY_PORT")
                .unwrap_or_else(|_| "3000".into())
                .parse()?,
            redis_url: std::env::var("REDIS_URL")?,
        })
    }

    pub async fn build_state(&self) -> Result<AppState> {
        let redis = redis::Client::open(self.redis_url.as_str())?;
        Ok(AppState { redis })
    }
}
