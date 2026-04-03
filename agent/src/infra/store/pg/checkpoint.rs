// ============================================================================
// PG Checkpoint — Warm Layer
//
// Durable backup for conversation snapshots.
// Written async after Redis (hot) save. Read on Redis miss.
// ============================================================================

use anyhow::{Context, Result};

use crate::infra::store::Store;

impl Store {
    /// Persist checkpoint to PG (upsert). Called async, non-blocking.
    pub(crate) async fn pg_save_checkpoint(&self, thread_id: &str, data: &[u8]) -> Result<()> {
        sqlx::query(
            "INSERT INTO checkpoints (thread_id, data, updated_at)
             VALUES ($1, $2, now())
             ON CONFLICT (thread_id) DO UPDATE SET data = $2, updated_at = now()",
        )
        .bind(thread_id)
        .bind(data)
        .execute(self.pg())
        .await
        .context(format!("failed to persist checkpoint for {thread_id}"))?;
        Ok(())
    }

    /// Load checkpoint from PG. Called when Redis misses.
    pub(crate) async fn pg_load_checkpoint(&self, thread_id: &str) -> Result<Option<Vec<u8>>> {
        let row: Option<(Vec<u8>,)> =
            sqlx::query_as("SELECT data FROM checkpoints WHERE thread_id = $1")
                .bind(thread_id)
                .fetch_optional(self.pg())
                .await
                .context(format!("failed to load checkpoint for {thread_id}"))?;

        Ok(row.map(|r| r.0))
    }
}
