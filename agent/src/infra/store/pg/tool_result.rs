// ============================================================================
// Tool Result Persistence
//
// When a tool result exceeds the inline size limit, the full content is
// stored here and the message carries a truncated preview + reference.
//
// On session resume, the reference is used to reconstruct the exact same
// truncation — keeping the LLM's prompt cache valid.
//
// Table: tool_results
//   thread_id    — which conversation
//   tool_use_id  — links to the tool_use block in the message
//   content      — full untruncated result
//   created_at   — for cleanup
// ============================================================================

#![allow(dead_code)]

use anyhow::{Context, Result};

use crate::infra::store::Store;

/// Maximum inline size for a tool result in the message.
/// Larger results are stored externally and replaced with a preview.
pub const MAX_INLINE_CHARS: usize = 50_000;

/// How many characters to show in the inline preview.
const PREVIEW_CHARS: usize = 5_000;

/// A reference to a persisted tool result.
#[derive(Debug, Clone)]
pub struct ToolResultRef {
    pub tool_use_id: String,
    pub preview: String,
    pub total_chars: usize,
}

impl Store {
    /// Persist a large tool result and return a truncated preview.
    ///
    /// If the content is under MAX_INLINE_CHARS, returns None (no persistence needed).
    pub async fn persist_tool_result(
        &self,
        thread_id: &str,
        tool_use_id: &str,
        content: &str,
    ) -> Result<Option<ToolResultRef>> {
        if content.len() <= MAX_INLINE_CHARS {
            return Ok(None);
        }

        sqlx::query(
            "INSERT INTO tool_results (thread_id, tool_use_id, content)
             VALUES ($1, $2, $3)
             ON CONFLICT (tool_use_id) DO UPDATE SET content = $3",
        )
        .bind(thread_id)
        .bind(tool_use_id)
        .bind(content)
        .execute(self.pg())
        .await
        .context("failed to persist tool result")?;

        let preview = format!(
            "{}\n\n[Result truncated. Showing first {} of {} chars. Full result stored.]",
            &content[..PREVIEW_CHARS.min(content.len())],
            PREVIEW_CHARS,
            content.len()
        );

        Ok(Some(ToolResultRef {
            tool_use_id: tool_use_id.to_string(),
            preview,
            total_chars: content.len(),
        }))
    }

    /// Load the full content of a persisted tool result.
    ///
    /// Used during session resume to reconstruct truncation decisions.
    pub async fn load_tool_result(&self, tool_use_id: &str) -> Result<Option<String>> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT content FROM tool_results WHERE tool_use_id = $1")
                .bind(tool_use_id)
                .fetch_optional(self.pg())
                .await
                .context("failed to load tool result")?;

        Ok(row.map(|r| r.0))
    }
}
