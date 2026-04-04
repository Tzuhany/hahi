// ============================================================================
// Memory Types
//
// "kind" is a free-form string — the LLM decides what to call a memory.
// The system has exactly one structural distinction: pinned vs. recalled.
//
// Pinned kinds ("identity", "feedback") are injected unconditionally every turn.
// All other kinds go through the RRF recall pipeline and appear only when relevant.
//
// "importance" [0.0, 1.0] evolves automatically via lifecycle:
//   - Frequently-recalled memories get boosted toward 1.0
//   - Untouched memories decay toward 0.0 and eventually retire
//   - This drives ordering without any manual curation
// ============================================================================

use serde::{Deserialize, Serialize};

/// Kinds that bypass the recall pipeline and are always present in context.
/// Everything else is conditionally recalled via RRF.
pub const PINNED_KINDS: &[&str] = &["identity", "feedback"];

/// Returns true if this kind bypasses the recall pipeline and is always in context.
#[cfg_attr(not(test), allow(dead_code))]
pub fn is_pinned(kind: &str) -> bool {
    PINNED_KINDS.contains(&kind)
}

/// A single memory entry (full, including body).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: String,
    pub agent_id: String,

    /// Free-form category decided by the LLM.
    /// Common values: "identity", "feedback", "experience", "decision", "fact", "reference"
    pub kind: String,

    /// Short title (3–8 words). Shown in the memory index every turn.
    pub name: String,

    /// Full content. Loaded when pinned or when recalled as relevant.
    pub body: String,

    /// Importance score in [0.0, 1.0]. Affects recall ranking. Updated by lifecycle.
    pub importance: f64,

    /// How many times this memory has been surfaced via recall or search.
    pub access_count: i64,

    pub accessed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// Optional expiry. None = permanent.
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Lightweight entry for the always-visible memory index in the system prompt.
/// Contains only the ID, kind, and name — no body.
#[derive(Debug, Clone)]
pub struct MemoryIndexEntry {
    /// The memory's ID. Kept for future operations (deletion, targeted recall)
    /// that need to reference a specific entry from the index.
    #[allow(dead_code)]
    pub id: String,
    pub kind: String,
    pub name: String,
}

impl MemoryIndexEntry {
    /// Single line for the system-prompt memory index.
    /// Example: `[feedback] no-emoji-in-responses`
    pub fn format_line(&self) -> String {
        format!("[{}] {}", self.kind, self.name)
    }
}

/// Input to the write pipeline. Validated by policy.rs before reaching the store.
#[derive(Debug, Clone)]
pub struct WriteRequest {
    pub agent_id: String,

    /// The kind the LLM chose. Validated for non-empty only; semantics are LLM's concern.
    pub kind: String,

    /// Short name for the index. Will be truncated if > NAME_LIMIT chars.
    pub name: String,

    /// Full content to persist.
    pub body: String,

    /// Optional TTL in days. None = permanent.
    pub ttl_days: Option<u32>,
}

/// Result of attempting to write a memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteStatus {
    /// Successfully written. Returns the new memory ID.
    Saved { id: String },

    /// Content hash matched an existing live memory.
    /// Nothing was written — no duplicate created.
    AlreadyKnown,
}

impl WriteStatus {
    /// Human-readable string returned to the LLM as tool output.
    pub fn to_tool_output(&self) -> String {
        match self {
            WriteStatus::Saved { id } => format!("memory saved (id: {id})"),
            WriteStatus::AlreadyKnown => {
                "already known — identical memory exists, no duplicate created".to_string()
            }
        }
    }
}

/// Combined output of one recall operation.
#[derive(Debug, Default)]
pub struct RecallResult {
    /// Pinned memories. Always present regardless of query.
    pub pinned: Vec<Memory>,

    /// Conditionally recalled memories, ranked by RRF × importance.
    pub recalled: Vec<Memory>,
}

impl RecallResult {
    pub fn is_empty(&self) -> bool {
        self.pinned.is_empty() && self.recalled.is_empty()
    }

    /// All memories in injection order: pinned first, then recalled.
    pub fn all(&self) -> impl Iterator<Item = &Memory> {
        self.pinned.iter().chain(self.recalled.iter())
    }
}

/// Session-level statistics used by reflect.rs to decide when to trigger reflection.
#[derive(Debug, Default, Clone)]
pub struct SessionStats {
    pub turn_count: u32,
    pub memories_written_this_run: u32,
    pub last_reflection_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pinned_kinds() {
        assert!(is_pinned("identity"));
        assert!(is_pinned("feedback"));
        assert!(!is_pinned("experience"));
        assert!(!is_pinned("decision"));
        assert!(!is_pinned("fact"));
        assert!(!is_pinned("reference"));
    }

    #[test]
    fn test_index_entry_format() {
        let e = MemoryIndexEntry {
            id: "1".into(),
            kind: "feedback".into(),
            name: "no-emoji-in-responses".into(),
        };
        assert_eq!(e.format_line(), "[feedback] no-emoji-in-responses");
    }

    #[test]
    fn test_write_status_output() {
        let s = WriteStatus::Saved {
            id: "abc-123".into(),
        };
        assert!(s.to_tool_output().contains("abc-123"));

        let s = WriteStatus::AlreadyKnown;
        assert!(s.to_tool_output().contains("already known"));
    }

    #[test]
    fn test_recall_result_all() {
        let mut r = RecallResult::default();
        r.pinned.push(Memory {
            id: "p1".into(),
            agent_id: "a".into(),
            kind: "feedback".into(),
            name: "n".into(),
            body: "b".into(),
            importance: 1.0,
            access_count: 0,
            accessed_at: None,
            created_at: chrono::Utc::now(),
            expires_at: None,
        });
        r.recalled.push(Memory {
            id: "r1".into(),
            agent_id: "a".into(),
            kind: "experience".into(),
            name: "n".into(),
            body: "b".into(),
            importance: 0.5,
            access_count: 2,
            accessed_at: None,
            created_at: chrono::Utc::now(),
            expires_at: None,
        });
        let all: Vec<_> = r.all().collect();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, "p1"); // pinned comes first
        assert_eq!(all[1].id, "r1");
    }
}
