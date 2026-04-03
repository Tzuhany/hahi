// ============================================================================
// Redis Checkpoint — Hot Layer
//
// Fast read/write for active conversations. 24h TTL.
// On miss, the public API (Store::load_checkpoint) falls back to PG.
// ============================================================================

use anyhow::{Context, Result};

use crate::infra::store::Store;

impl Store {
    /// Save checkpoint bytes to Redis with 24h TTL.
    pub(crate) async fn redis_save_checkpoint(&self, thread_id: &str, data: &[u8]) -> Result<()> {
        let key = checkpoint_key(thread_id);
        let mut conn = self.redis_conn().await?;

        redis::cmd("SET")
            .arg(&key)
            .arg(data)
            .arg("EX")
            .arg(86400u64)
            .query_async::<()>(&mut conn)
            .await
            .context("failed to save checkpoint to Redis")?;

        Ok(())
    }

    /// Load checkpoint bytes from Redis. Returns None on miss.
    pub(crate) async fn redis_load_checkpoint(&self, thread_id: &str) -> Result<Option<Vec<u8>>> {
        let key = checkpoint_key(thread_id);
        let mut conn = self.redis_conn().await?;

        let data: Option<Vec<u8>> = redis::cmd("GET")
            .arg(&key)
            .query_async(&mut conn)
            .await
            .context("failed to load checkpoint from Redis")?;

        Ok(data)
    }
}

fn checkpoint_key(thread_id: &str) -> String {
    format!("checkpoint:{thread_id}")
}
