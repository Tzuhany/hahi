// ============================================================================
// Store — Unified Storage Layer
//
// ALL database and cache operations live here.
//
// store/
// ├── mod.rs             Store handle + fork logic
// ├── pg/
// │   ├── checkpoint.rs  PG checkpoint UPSERT/SELECT
// │   ├── memory.rs      PG memories CRUD + pgvector
// │   ├── tool_result.rs PG large tool result persistence
// │   └── audit.rs       PG execution audit trail
// └── redis/
//     ├── checkpoint.rs  Redis checkpoint SET/GET (hot cache)
//     └── event.rs       Redis Stream XADD (event output)
//
// Data types (Checkpoint, ForkOrigin) live in common/checkpoint.rs.
// ============================================================================

pub mod pg;
pub mod redis;

use anyhow::{Context, Result};

use crate::common::checkpoint::{Checkpoint, ForkOrigin};

/// The unified store handle. Constructed once, shared via Arc.
pub struct Store {
    pub(crate) pg: sqlx::PgPool,
    pub(crate) redis: ::redis::Client,
}

impl Store {
    /// Connect to PG and Redis. Fails fast if either is unreachable.
    pub async fn connect(database_url: &str, redis_url: &str) -> Result<Self> {
        let pg = sqlx::PgPool::connect(database_url)
            .await
            .context("failed to connect to PostgreSQL")?;

        let redis = ::redis::Client::open(redis_url).context("invalid Redis URL")?;

        let mut conn = redis
            .get_multiplexed_async_connection()
            .await
            .context("failed to connect to Redis")?;
        ::redis::cmd("PING")
            .query_async::<String>(&mut conn)
            .await
            .context("Redis ping failed")?;

        let store = Self { pg, redis };
        store.pg_ensure_runtime_state().await?;

        tracing::info!("store connected");
        Ok(store)
    }

    /// Fork a checkpoint: create a new thread that branches from an existing one.
    ///
    /// Copies the source checkpoint to a new thread_id, recording the fork origin.
    /// The new thread starts with the same history up to fork_point.
    #[allow(dead_code)]
    pub async fn fork_checkpoint(
        &self,
        source_thread_id: &str,
        new_thread_id: &str,
        fork_point_message_id: &str,
    ) -> Result<Checkpoint> {
        // Load the source checkpoint.
        let source_bytes = self.redis_load_checkpoint(source_thread_id).await?;
        let source_bytes = match source_bytes {
            Some(b) => b,
            None => self
                .pg_load_checkpoint(source_thread_id)
                .await?
                .context(format!("no checkpoint found for {source_thread_id}"))?,
        };

        let mut checkpoint: Checkpoint = serde_json::from_slice(&source_bytes)
            .context("failed to deserialize source checkpoint")?;

        // Set fork metadata.
        checkpoint.thread_id = new_thread_id.to_string();
        checkpoint.forked_from = Some(ForkOrigin {
            source_thread_id: source_thread_id.to_string(),
            fork_point_message_id: fork_point_message_id.to_string(),
        });

        // Save the forked checkpoint.
        let data = serde_json::to_vec(&checkpoint)?;
        self.redis_save_checkpoint(new_thread_id, &data).await?;

        // PG persist (async).
        let pg = self.pg.clone();
        let tid = new_thread_id.to_string();
        let data_clone = data.clone();
        tokio::spawn(async move {
            let _ = sqlx::query(
                "INSERT INTO checkpoints (thread_id, data, updated_at)
                 VALUES ($1, $2, now())
                 ON CONFLICT (thread_id) DO UPDATE SET data = $2, updated_at = now()",
            )
            .bind(&tid)
            .bind(&data_clone)
            .execute(&pg)
            .await;
        });

        tracing::info!(
            source = source_thread_id,
            fork = new_thread_id,
            fork_point = fork_point_message_id,
            "checkpoint forked"
        );

        Ok(checkpoint)
    }
}
