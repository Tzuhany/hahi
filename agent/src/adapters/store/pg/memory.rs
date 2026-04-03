// ============================================================================
// PG Memory Store
//
// All SQL for the memories table. Schema (run once at setup):
//
//   CREATE EXTENSION IF NOT EXISTS vector;
//
//   CREATE TABLE memories (
//     id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
//     agent_id      TEXT        NOT NULL,
//     kind          TEXT        NOT NULL,
//     name          TEXT        NOT NULL,
//     body          TEXT        NOT NULL,
//     content_hash  TEXT        NOT NULL,  -- sha256(body), for dedup
//     embedding     VECTOR(1536),          -- null until filled by background job
//     search_vec    TSVECTOR    GENERATED ALWAYS AS
//                               (to_tsvector('simple', name || ' ' || body)) STORED,
//     importance    FLOAT8      NOT NULL DEFAULT 0.5,
//     access_count  INTEGER     NOT NULL DEFAULT 0,
//     accessed_at   TIMESTAMPTZ,
//     expires_at    TIMESTAMPTZ,
//     retired_at    TIMESTAMPTZ,
//     retired_reason TEXT,
//     created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
//     updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
//   );
//
//   -- Dedup: same content cannot appear twice for the same agent.
//   CREATE UNIQUE INDEX memories_dedup
//     ON memories (agent_id, content_hash)
//     WHERE retired_at IS NULL;
//
//   -- Fast pinned lookup.
//   CREATE INDEX memories_pinned
//     ON memories (agent_id)
//     WHERE kind IN ('identity', 'feedback') AND retired_at IS NULL;
//
//   -- Lexical search.
//   CREATE INDEX memories_search
//     ON memories USING GIN (search_vec)
//     WHERE retired_at IS NULL;
//
//   -- Vector search (HNSW for approximate nearest neighbor).
//   CREATE INDEX memories_embedding
//     ON memories USING hnsw (embedding vector_cosine_ops)
//     WHERE retired_at IS NULL AND embedding IS NOT NULL;
//
//   -- Lifecycle queries.
//   CREATE INDEX memories_lifecycle
//     ON memories (agent_id, access_count, created_at)
//     WHERE retired_at IS NULL;
//
// ============================================================================

#![allow(dead_code)]

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::adapters::store::Store;
use crate::systems::memory::policy::ValidatedWrite;
use crate::systems::memory::types::{Memory, MemoryIndexEntry, WriteStatus};

// ────────────────────────────────────────────────────────────────────────────
// Row types
// ────────────────────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct MemoryRow {
    id: uuid::Uuid,
    agent_id: String,
    kind: String,
    name: String,
    body: String,
    importance: f64,
    access_count: i64,
    accessed_at: Option<chrono::DateTime<chrono::Utc>>,
    created_at: chrono::DateTime<chrono::Utc>,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<MemoryRow> for Memory {
    fn from(r: MemoryRow) -> Self {
        Memory {
            id: r.id.to_string(),
            agent_id: r.agent_id,
            kind: r.kind,
            name: r.name,
            body: r.body,
            importance: r.importance,
            access_count: r.access_count,
            accessed_at: r.accessed_at,
            created_at: r.created_at,
            expires_at: r.expires_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct IndexRow {
    id: uuid::Uuid,
    kind: String,
    name: String,
}

impl From<IndexRow> for MemoryIndexEntry {
    fn from(r: IndexRow) -> Self {
        MemoryIndexEntry {
            id: r.id.to_string(),
            kind: r.kind,
            name: r.name,
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────────────────────

fn sha256_hex(text: &str) -> String {
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    format!("{:x}", h.finalize())
}

// ────────────────────────────────────────────────────────────────────────────
// Store impls
// ────────────────────────────────────────────────────────────────────────────

impl Store {
    // ── Write ────────────────────────────────────────────────────────────────

    /// Write a validated memory, respecting the content-hash unique index.
    ///
    /// Returns `WriteStatus::AlreadyKnown` when an identical memory already
    /// exists for this agent (ON CONFLICT DO NOTHING).
    pub async fn memory_write(
        &self,
        req: &ValidatedWrite,
        embedding: Option<&[f32]>,
    ) -> Result<WriteStatus> {
        let id = uuid::Uuid::new_v4();
        let hash = sha256_hex(&req.body);
        let expires_at = req
            .ttl_days
            .map(|d| chrono::Utc::now() + chrono::Duration::days(d as i64));

        let rows_affected = sqlx::query(
            "INSERT INTO memories
               (id, agent_id, kind, name, body, content_hash, embedding, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (agent_id, content_hash)
             WHERE retired_at IS NULL
             DO NOTHING",
        )
        .bind(id)
        .bind(&req.agent_id)
        .bind(&req.kind)
        .bind(&req.name)
        .bind(&req.body)
        .bind(&hash)
        .bind(embedding.map(|e| pgvector::Vector::from(e.to_vec())))
        .bind(expires_at)
        .execute(self.pg())
        .await
        .context("failed to write memory")?
        .rows_affected();

        if rows_affected == 0 {
            tracing::debug!(
                agent_id = req.agent_id,
                name = req.name,
                "memory already known"
            );
            Ok(WriteStatus::AlreadyKnown)
        } else {
            tracing::info!(
                agent_id = req.agent_id,
                id = %id,
                kind = req.kind,
                name = req.name,
                "memory written"
            );
            Ok(WriteStatus::Saved { id: id.to_string() })
        }
    }

    // ── Recall ───────────────────────────────────────────────────────────────

    /// Load all pinned memories (kind IN ('identity', 'feedback')).
    /// Always called; result is always injected into context.
    pub async fn memory_recall_pinned(&self, agent_id: &str) -> Result<Vec<Memory>> {
        let rows = sqlx::query_as::<_, MemoryRow>(
            "SELECT id, agent_id, kind, name, body, importance, access_count,
                    accessed_at, created_at, expires_at
             FROM memories
             WHERE agent_id = $1
               AND kind = ANY($2)
               AND retired_at IS NULL
               AND (expires_at IS NULL OR expires_at > now())
             ORDER BY importance DESC, created_at DESC",
        )
        .bind(agent_id)
        .bind(&["identity", "feedback"] as &[&str])
        .fetch_all(self.pg())
        .await
        .context("failed to recall pinned memories")?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    /// Hybrid RRF recall for non-pinned memories.
    ///
    /// Combines lexical (plainto_tsquery) and vector (pgvector cosine distance)
    /// rankings via Reciprocal Rank Fusion (k=60). Score is multiplied by
    /// importance so higher-quality memories naturally surface.
    ///
    /// If `embedding` is None (no embedder configured), falls back to
    /// lexical-only. If `query` is empty, returns recent memories by importance.
    pub async fn memory_recall_conditional(
        &self,
        agent_id: &str,
        query: &str,
        embedding: Option<&[f32]>,
        limit: i64,
    ) -> Result<Vec<Memory>> {
        // Empty query: return recent memories ordered by importance.
        if query.trim().is_empty() && embedding.is_none() {
            return self.memory_recent(agent_id, limit).await;
        }

        let rows = sqlx::query_as::<_, MemoryRow>(
            r#"
            WITH base AS (
                SELECT id, agent_id, kind, name, body, importance, access_count,
                       accessed_at, created_at, expires_at
                FROM memories
                WHERE agent_id = $1
                  AND kind != ALL($2)
                  AND retired_at IS NULL
                  AND (expires_at IS NULL OR expires_at > now())
            ),
            lexical AS (
                SELECT b.id,
                       ROW_NUMBER() OVER (
                           ORDER BY ts_rank(m.search_vec,
                                            plainto_tsquery('simple', $3)) DESC
                       ) AS lex_rank
                FROM base b
                JOIN memories m ON m.id = b.id
                WHERE $3 != ''
                  AND m.search_vec @@ plainto_tsquery('simple', $3)
                LIMIT 30
            ),
            vector AS (
                SELECT b.id,
                       ROW_NUMBER() OVER (
                           ORDER BY m.embedding <=> $4::vector
                       ) AS vec_rank
                FROM base b
                JOIN memories m ON m.id = b.id
                WHERE $4::vector IS NOT NULL
                  AND m.embedding IS NOT NULL
                LIMIT 30
            ),
            rrf AS (
                SELECT
                    COALESCE(l.id, v.id) AS id,
                    COALESCE(1.0 / (60.0 + l.lex_rank), 0.0) +
                    COALESCE(1.0 / (60.0 + v.vec_rank), 0.0) AS rrf_score
                FROM lexical l
                FULL OUTER JOIN vector v ON l.id = v.id
            )
            SELECT b.id, b.agent_id, b.kind, b.name, b.body,
                   b.importance, b.access_count, b.accessed_at,
                   b.created_at, b.expires_at
            FROM base b
            JOIN rrf r ON r.id = b.id
            ORDER BY r.rrf_score * b.importance DESC
            LIMIT $5
            "#,
        )
        .bind(agent_id)
        .bind(&["identity", "feedback"] as &[&str])
        .bind(if query.trim().is_empty() { "" } else { query })
        .bind(embedding.map(|e| pgvector::Vector::from(e.to_vec())))
        .bind(limit)
        .fetch_all(self.pg())
        .await
        .context("failed to recall conditional memories")?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    /// Fallback when there's no query: most important recent memories.
    async fn memory_recent(&self, agent_id: &str, limit: i64) -> Result<Vec<Memory>> {
        let rows = sqlx::query_as::<_, MemoryRow>(
            "SELECT id, agent_id, kind, name, body, importance, access_count,
                    accessed_at, created_at, expires_at
             FROM memories
             WHERE agent_id = $1
               AND kind != ALL($2)
               AND retired_at IS NULL
               AND (expires_at IS NULL OR expires_at > now())
             ORDER BY importance DESC, created_at DESC
             LIMIT $3",
        )
        .bind(agent_id)
        .bind(&["identity", "feedback"] as &[&str])
        .bind(limit)
        .fetch_all(self.pg())
        .await
        .context("failed to recall recent memories")?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    /// Update access_count and accessed_at for a set of memory IDs.
    /// Called asynchronously after recall — does not block the turn.
    pub async fn memory_record_access(&self, ids: &[String]) -> Result<()> {
        // Parse to UUIDs for type safety.
        let uuids: Vec<uuid::Uuid> = ids.iter().filter_map(|s| s.parse().ok()).collect();

        if uuids.is_empty() {
            return Ok(());
        }

        sqlx::query(
            "UPDATE memories
             SET access_count = access_count + 1,
                 accessed_at  = now()
             WHERE id = ANY($1)",
        )
        .bind(&uuids)
        .execute(self.pg())
        .await
        .context("failed to record memory access")?;

        Ok(())
    }

    // ── Index ────────────────────────────────────────────────────────────────

    /// List all non-retired memories as lightweight index entries.
    ///
    /// Ordering: pinned kinds first (feedback → identity), then by importance
    /// descending. This is what populates the system-prompt memory index.
    pub async fn memory_list_index(&self, agent_id: &str) -> Result<Vec<MemoryIndexEntry>> {
        let rows = sqlx::query_as::<_, IndexRow>(
            "SELECT id, kind, name
             FROM memories
             WHERE agent_id = $1
               AND retired_at IS NULL
               AND (expires_at IS NULL OR expires_at > now())
             ORDER BY
               CASE kind
                 WHEN 'feedback' THEN 0
                 WHEN 'identity' THEN 1
                 ELSE 2
               END,
               importance DESC,
               created_at DESC
             LIMIT 200",
        )
        .bind(agent_id)
        .fetch_all(self.pg())
        .await
        .context("failed to list memory index")?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    // ── Search (explicit tool call) ──────────────────────────────────────────

    /// Explicit search invoked by the MemorySearch tool.
    ///
    /// Same RRF logic as recall_conditional but searches ALL non-retired
    /// memories (not just non-pinned), and returns more results.
    pub async fn memory_search(
        &self,
        agent_id: &str,
        query: &str,
        embedding: Option<&[f32]>,
    ) -> Result<Vec<Memory>> {
        const SEARCH_LIMIT: i64 = 5;

        if query.trim().is_empty() {
            return Ok(vec![]);
        }

        let rows = sqlx::query_as::<_, MemoryRow>(
            r#"
            WITH lexical AS (
                SELECT m.id,
                       ROW_NUMBER() OVER (
                           ORDER BY ts_rank(m.search_vec,
                                            plainto_tsquery('simple', $2)) DESC
                       ) AS lex_rank
                FROM memories m
                WHERE m.agent_id = $1
                  AND m.retired_at IS NULL
                  AND m.search_vec @@ plainto_tsquery('simple', $2)
                LIMIT 20
            ),
            vector AS (
                SELECT m.id,
                       ROW_NUMBER() OVER (
                           ORDER BY m.embedding <=> $3::vector
                       ) AS vec_rank
                FROM memories m
                WHERE m.agent_id = $1
                  AND m.retired_at IS NULL
                  AND m.embedding IS NOT NULL
                  AND $3::vector IS NOT NULL
                LIMIT 20
            ),
            rrf AS (
                SELECT
                    COALESCE(l.id, v.id) AS id,
                    COALESCE(1.0 / (60.0 + l.lex_rank), 0.0) +
                    COALESCE(1.0 / (60.0 + v.vec_rank), 0.0) AS rrf_score
                FROM lexical l
                FULL OUTER JOIN vector v ON l.id = v.id
            )
            SELECT m.id, m.agent_id, m.kind, m.name, m.body,
                   m.importance, m.access_count, m.accessed_at,
                   m.created_at, m.expires_at
            FROM memories m
            JOIN rrf r ON r.id = m.id
            ORDER BY r.rrf_score * m.importance DESC
            LIMIT $4
            "#,
        )
        .bind(agent_id)
        .bind(query)
        .bind(embedding.map(|e| pgvector::Vector::from(e.to_vec())))
        .bind(SEARCH_LIMIT)
        .fetch_all(self.pg())
        .await
        .context("failed to search memories")?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    // ── Forget ───────────────────────────────────────────────────────────────

    /// Soft-delete a memory (called by the MemoryForget tool).
    pub async fn memory_retire(&self, agent_id: &str, memory_id: &str, reason: &str) -> Result<()> {
        let id: uuid::Uuid = memory_id
            .parse()
            .with_context(|| format!("invalid memory id: {memory_id}"))?;

        let rows = sqlx::query(
            "UPDATE memories
             SET retired_at = now(), retired_reason = $3
             WHERE id = $1 AND agent_id = $2 AND retired_at IS NULL",
        )
        .bind(id)
        .bind(agent_id)
        .bind(reason)
        .execute(self.pg())
        .await
        .context("failed to retire memory")?
        .rows_affected();

        anyhow::ensure!(rows > 0, "memory {memory_id} not found or already retired");
        tracing::info!(agent_id, memory_id, reason, "memory retired");
        Ok(())
    }

    // ── Lifecycle ────────────────────────────────────────────────────────────

    /// Retire non-pinned memories that have never been accessed after `stale_days`.
    pub async fn memory_retire_stale(&self, agent_id: &str, stale_days: i64) -> Result<u64> {
        let rows = sqlx::query(
            "UPDATE memories
             SET retired_at = now(), retired_reason = 'stale'
             WHERE agent_id = $1
               AND kind != ALL($2)
               AND retired_at IS NULL
               AND access_count = 0
               AND created_at < now() - ($3 * interval '1 day')",
        )
        .bind(agent_id)
        .bind(&["identity", "feedback"] as &[&str])
        .bind(stale_days)
        .execute(self.pg())
        .await
        .context("failed to retire stale memories")?
        .rows_affected();

        Ok(rows)
    }

    /// Multiply importance by `factor` for memories not accessed in `days`.
    /// Used for decay (factor < 1.0).
    pub async fn memory_decay_importance(&self, agent_id: &str, factor: f64) -> Result<u64> {
        let rows = sqlx::query(
            "UPDATE memories
             SET importance = GREATEST(0.0, importance * $3),
                 updated_at = now()
             WHERE agent_id = $1
               AND kind != ALL($2)
               AND retired_at IS NULL
               AND (accessed_at IS NULL OR accessed_at < now() - interval '7 days')",
        )
        .bind(agent_id)
        .bind(&["identity", "feedback"] as &[&str])
        .bind(factor)
        .execute(self.pg())
        .await
        .context("failed to decay memory importance")?
        .rows_affected();

        Ok(rows)
    }

    /// Boost importance for frequently-accessed memories (factor > 1.0).
    pub async fn memory_boost_importance(
        &self,
        agent_id: &str,
        factor: f64,
        min_accesses: i64,
        recent_days: i64,
    ) -> Result<u64> {
        let rows = sqlx::query(
            "UPDATE memories
             SET importance = LEAST(1.0, importance * $3),
                 updated_at = now()
             WHERE agent_id = $1
               AND retired_at IS NULL
               AND access_count >= $4
               AND accessed_at > now() - ($5 * interval '1 day')",
        )
        .bind(agent_id)
        .bind(&[] as &[&str]) // all kinds eligible for boost, including pinned
        .bind(factor)
        .bind(min_accesses)
        .bind(recent_days)
        .execute(self.pg())
        .await
        .context("failed to boost memory importance")?
        .rows_affected();

        Ok(rows)
    }

    /// Store an embedding for a memory (called by background embedding job).
    pub async fn memory_set_embedding(&self, memory_id: &str, embedding: &[f32]) -> Result<()> {
        let id: uuid::Uuid = memory_id
            .parse()
            .with_context(|| format!("invalid memory id: {memory_id}"))?;

        sqlx::query("UPDATE memories SET embedding = $2, updated_at = now() WHERE id = $1")
            .bind(id)
            .bind(pgvector::Vector::from(embedding.to_vec()))
            .execute(self.pg())
            .await
            .context("failed to set memory embedding")?;

        Ok(())
    }

    /// Return IDs of memories that have no embedding yet (for background filling).
    pub async fn memory_ids_without_embedding(
        &self,
        agent_id: &str,
        limit: i64,
    ) -> Result<Vec<String>> {
        let rows: Vec<(uuid::Uuid,)> = sqlx::query_as(
            "SELECT id FROM memories
             WHERE agent_id = $1
               AND embedding IS NULL
               AND retired_at IS NULL
             LIMIT $2",
        )
        .bind(agent_id)
        .bind(limit)
        .fetch_all(self.pg())
        .await
        .context("failed to list memories without embeddings")?;

        Ok(rows.into_iter().map(|(id,)| id.to_string()).collect())
    }
}
