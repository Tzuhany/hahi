use anyhow::Result;

pub struct Config {
    pub addr: String,
    pub database_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            addr: std::env::var("Uchat_SERVICE_ADDR")?,
            database_url: std::env::var("DATABASE_URL")?,
        })
    }
}
