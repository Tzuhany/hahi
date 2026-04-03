// ============================================================================
// PG Runtime State — Cross-Thread Agent Policy State
//
// Checkpoints are thread-scoped execution snapshots. Cross-thread policy state
// that belongs to an agent/user pair lives here instead.
//
// Current fields:
//   memory_runtime_state.last_reflection_at
// ============================================================================

use anyhow::{Context, Result};

use crate::infra::store::Store;

impl Store {
    /// Ensure the runtime-state table exists before the agent starts serving.
    pub(crate) async fn pg_ensure_runtime_state(&self) -> Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS memory_runtime_state (
                 agent_id TEXT PRIMARY KEY,
                 last_reflection_at TIMESTAMPTZ,
                 updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
             )",
        )
        .execute(self.pg())
        .await
        .context("failed to ensure memory_runtime_state table")?;
        Ok(())
    }

    /// Load the last successful reflection timestamp for an agent/user.
    pub async fn load_last_reflection_at(
        &self,
        agent_id: &str,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
        let row: Option<(Option<chrono::DateTime<chrono::Utc>>,)> = sqlx::query_as(
            "SELECT last_reflection_at
             FROM memory_runtime_state
             WHERE agent_id = $1",
        )
        .bind(agent_id)
        .fetch_optional(self.pg())
        .await
        .context(format!(
            "failed to load memory runtime state for agent '{agent_id}'"
        ))?;

        Ok(row.and_then(|r| r.0))
    }

    /// Persist the last successful reflection timestamp for an agent/user.
    pub async fn save_last_reflection_at(
        &self,
        agent_id: &str,
        timestamp: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO memory_runtime_state (agent_id, last_reflection_at, updated_at)
             VALUES ($1, $2, now())
             ON CONFLICT (agent_id) DO UPDATE
             SET last_reflection_at = $2,
                 updated_at = now()",
        )
        .bind(agent_id)
        .bind(timestamp)
        .execute(self.pg())
        .await
        .context(format!(
            "failed to save memory runtime state for agent '{agent_id}'"
        ))?;
        Ok(())
    }
}
