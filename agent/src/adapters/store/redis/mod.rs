// ============================================================================
// Redis — Hot Storage + Event Streaming
//
// Two concerns:
//   checkpoint.rs — Conversation snapshots (hot cache, 24h TTL)
//   event.rs      — Streaming events to Redis Stream (Gateway subscribes)
// ============================================================================

pub mod checkpoint;
pub mod event;

use anyhow::{Context, Result};

use crate::adapters::store::Store;

impl Store {
    /// Internal: get a multiplexed async Redis connection.
    pub(crate) async fn redis_conn(&self) -> Result<::redis::aio::MultiplexedConnection> {
        self.redis
            .get_multiplexed_async_connection()
            .await
            .context("failed to get Redis connection")
    }
}
