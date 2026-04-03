// ============================================================================
// Redis Event Stream — Real-time Event Output
//
// Writes agent execution events to Redis Stream.
// Gateway subscribes and pushes to clients via SSE.
//
// Stream key: results:{thread_id}
// Each event: XADD with type + JSON payload
// Auto-trimmed to ~2000 entries per stream.
// ============================================================================

use anyhow::{Context, Result};

use crate::adapters::store::Store;

impl Store {
    /// Write a single event to the Redis Stream.
    pub async fn emit_event(
        &self,
        thread_id: &str,
        event_type: &str,
        payload: &serde_json::Value,
    ) -> Result<String> {
        let key = stream_key(thread_id);
        let data = serde_json::to_string(payload).context("failed to serialize event")?;

        let mut conn = self.redis_conn().await?;

        let id: String = redis::cmd("XADD")
            .arg(&key)
            .arg("MAXLEN")
            .arg("~")
            .arg(2000)
            .arg("*")
            .arg("type")
            .arg(event_type)
            .arg("data")
            .arg(&data)
            .query_async(&mut conn)
            .await
            .context("failed to write event to Redis Stream")?;

        Ok(id)
    }

    /// Set TTL on a stream (call after run completes).
    pub async fn expire_event_stream(&self, thread_id: &str, ttl_seconds: u64) -> Result<()> {
        let key = stream_key(thread_id);
        let mut conn = self.redis_conn().await?;

        redis::cmd("EXPIRE")
            .arg(&key)
            .arg(ttl_seconds)
            .query_async::<()>(&mut conn)
            .await
            .context("failed to set stream TTL")?;

        Ok(())
    }
}

fn stream_key(thread_id: &str) -> String {
    format!("results:{thread_id}")
}
