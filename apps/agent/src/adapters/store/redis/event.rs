// ============================================================================
// Redis Event Stream — Real-time Event Output
//
// Writes agent execution events to Redis Stream.
// Session subscribes, translates to typed session events, and Gateway pushes
// those onward via SSE.
//
// Stream key: results:{run_id}
// Each event: XADD with a small CloudEvents-style envelope:
//   specversion, source, type, time, datacontenttype, data
// Auto-trimmed to ~2000 entries per stream.
// ============================================================================

use anyhow::{Context, Result};
use chrono::Utc;

use crate::adapters::store::Store;

impl Store {
    /// Write a single event to the Redis Stream.
    pub async fn emit_event(
        &self,
        run_id: &str,
        event_type: &str,
        payload: &serde_json::Value,
    ) -> Result<String> {
        let key = stream_key(run_id);
        let data = serde_json::to_string(payload).context("failed to serialize event")?;

        let mut conn = self.redis_conn().await?;

        let id: String = redis::cmd("XADD")
            .arg(&key)
            .arg("MAXLEN")
            .arg("~")
            .arg(2000)
            .arg("*")
            .arg("specversion")
            .arg("1.0")
            .arg("source")
            .arg("agent")
            .arg("type")
            .arg(event_type)
            .arg("time")
            .arg(Utc::now().to_rfc3339())
            .arg("datacontenttype")
            .arg("application/json")
            .arg("data")
            .arg(&data)
            .query_async(&mut conn)
            .await
            .context("failed to write event to Redis Stream")?;

        Ok(id)
    }

    /// Set TTL on a stream (call after run completes).
    pub async fn expire_event_stream(&self, run_id: &str, ttl_seconds: u64) -> Result<()> {
        let key = stream_key(run_id);
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

fn stream_key(run_id: &str) -> String {
    format!("results:{run_id}")
}
