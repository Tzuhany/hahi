// ============================================================================
// Execution Audit Trail
//
// Records every significant action the agent takes during a run.
// Not the same as RunSteps (which Conversation module owns) — this is the
// agent's own operational log, stored in its own PG tables.
//
// Use cases:
//   - "What did the agent do?" — replay the audit log
//   - Compliance: prove the agent didn't access unauthorized resources
//   - Debugging: why did the agent make that decision?
//   - Analytics: which tools are used most, error rates, latency
//
// This is the generalized version of Claude Code's attribution tracking.
// Not git-specific — works for any operation (API calls, DB queries, etc.).
// ============================================================================

#![allow(dead_code)]

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::infra::store::Store;

/// An entry in the audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Which thread this happened in.
    pub thread_id: String,

    /// What happened.
    pub action: AuditAction,

    /// When it happened.
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// What the agent did.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuditAction {
    /// Agent called a tool.
    ToolCall {
        tool_name: String,
        input_preview: String,
        is_error: bool,
        duration_ms: u64,
    },

    /// Agent wrote a new memory.
    MemoryWrite {
        memory_id: String,
        memory_type: String,
        name: String,
    },

    /// Agent compacted context.
    ContextCompact {
        pre_tokens: u64,
        post_tokens: u64,
        strategy: String, // "collapse" or "summarize"
    },

    /// Agent spawned a sub-agent.
    SubAgentSpawn { agent_type: String, depth: u32 },

    /// Agent entered/exited plan mode.
    PlanModeChange { new_mode: String },

    /// Permission was requested/granted/denied.
    PermissionCheck { tool_name: String, decision: String },
}

impl Store {
    /// Append an audit entry. Fire-and-forget (async, non-blocking).
    pub fn audit(&self, entry: AuditEntry) {
        let pg = self.pg.clone();
        let data = match serde_json::to_value(&entry) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "failed to serialize audit entry");
                return;
            }
        };

        tokio::spawn(async move {
            if let Err(e) = sqlx::query(
                "INSERT INTO audit_log (thread_id, action, timestamp)
                 VALUES ($1, $2, $3)",
            )
            .bind(&entry.thread_id)
            .bind(&data)
            .bind(entry.timestamp)
            .execute(&pg)
            .await
            {
                tracing::warn!(error = %e, "failed to write audit entry");
            }
        });
    }

    /// Load audit trail for a thread.
    pub async fn load_audit_trail(&self, thread_id: &str, limit: i64) -> Result<Vec<AuditEntry>> {
        let rows: Vec<(String, serde_json::Value, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
            "SELECT thread_id, action, timestamp FROM audit_log
                 WHERE thread_id = $1
                 ORDER BY timestamp DESC
                 LIMIT $2",
        )
        .bind(thread_id)
        .bind(limit)
        .fetch_all(self.pg())
        .await
        .context("failed to load audit trail")?;

        let entries = rows
            .into_iter()
            .filter_map(|(tid, action, ts)| {
                let action: AuditAction = serde_json::from_value(action).ok()?;
                Some(AuditEntry {
                    thread_id: tid,
                    action,
                    timestamp: ts,
                })
            })
            .collect();

        Ok(entries)
    }
}
