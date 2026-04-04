// ============================================================================
// Memory Injection
//
// Two injection points per turn:
//
//   1. System prompt — the memory index (always present, lightweight)
//      Format: one `[kind] name` line per memory, ≤ MAX_INDEX_ENTRIES lines.
//      Purpose: LLM sees what it knows without seeing the full content.
//              It can call MemorySearch if it needs something not yet recalled.
//
//   2. Turn start — recalled memories (full body, wrapped in <memory> tags)
//      Format: <memory kind="feedback">body</memory>
//      Purpose: LLM has directly relevant context without needing a tool call.
//
// Pinned memories appear in both the index AND the recalled section.
// Conditional memories appear in the index always, in recalled only when surfaced.
// ============================================================================

use crate::kernel::xml;
use crate::systems::memory::types::{Memory, MemoryIndexEntry, RecallResult};

/// Max entries in the system-prompt memory index.
/// Beyond this the index itself starts to bloat the prompt.
const MAX_INDEX_ENTRIES: usize = 100;

/// Format the memory index for the system prompt.
///
/// Returns None if no memories exist yet (new agent).
/// Returns a formatted block with one line per memory if memories exist.
pub fn format_index(entries: &[MemoryIndexEntry]) -> Option<String> {
    if entries.is_empty() {
        return None;
    }

    let visible = entries.len().min(MAX_INDEX_ENTRIES);
    let overflow = entries.len().saturating_sub(MAX_INDEX_ENTRIES);

    let mut lines: Vec<String> = entries[..visible]
        .iter()
        .map(|e| format!("  {}", e.format_line()))
        .collect();

    if overflow > 0 {
        lines.push(format!(
            "  ... and {overflow} more (use MemorySearch to find them)"
        ));
    }

    Some(format!(
        "## Memory\n\
         You have persistent memory across sessions.\n\
         Index ({total} entries):\n\
         {lines}\n\n\
         Use MemoryWrite to save new memories, MemorySearch to find specific ones,\n\
         and MemoryForget to remove outdated ones.",
        total = entries.len(),
        lines = lines.join("\n"),
    ))
}

/// Format recalled memories for injection at the start of a turn.
///
/// Pinned and recalled memories are injected together as a `<system-reminder>`.
/// Returns None if the RecallResult is empty.
pub fn format_recalled(result: &RecallResult) -> Option<String> {
    if result.is_empty() {
        return None;
    }

    let sections: Vec<String> = result.all().map(|m| format_memory_block(m)).collect();

    let inner = format!(
        "Relevant memories for this turn:\n\n{}",
        sections.join("\n\n")
    );

    Some(xml::system_reminder(&inner))
}

/// Format a single memory as an XML block for in-turn injection.
fn format_memory_block(m: &Memory) -> String {
    format!(
        "<memory kind=\"{}\" id=\"{}\">\n{}\n</memory>",
        m.kind,
        m.id,
        m.body.trim()
    )
}

/// Format the "what NOT to save" constraint block for the system prompt.
///
/// Injected alongside the memory index to guide the LLM's write decisions.
pub fn format_write_guidance() -> &'static str {
    "## Memory write guidance\n\
     Save memories that are:\n\
     - Behavioral corrections (\"don't do X\", \"always do Y\")\n\
     - Durable facts about the user (role, preferences, goals)\n\
     - Important decisions and their reasoning\n\
     - References to external systems the user mentioned\n\n\
     Do NOT save:\n\
     - Anything derivable from the codebase or documentation\n\
     - Task-specific details that won't matter next session\n\
     - Information already in your memory index\n\
     - Intermediate steps or temporary state\n\n\
     When unsure, don't save. Quality over quantity."
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(kind: &str, name: &str) -> MemoryIndexEntry {
        MemoryIndexEntry {
            id: "1".into(),
            kind: kind.into(),
            name: name.into(),
        }
    }

    fn make_memory(kind: &str, id: &str, body: &str) -> Memory {
        Memory {
            id: id.into(),
            agent_id: "a".into(),
            kind: kind.into(),
            name: "test".into(),
            body: body.into(),
            importance: 0.5,
            access_count: 0,
            accessed_at: None,
            created_at: chrono::Utc::now(),
            expires_at: None,
        }
    }

    #[test]
    fn test_format_index_empty() {
        assert!(format_index(&[]).is_none());
    }

    #[test]
    fn test_format_index_single() {
        let entries = vec![make_entry("feedback", "no-emoji")];
        let out = format_index(&entries).unwrap();
        assert!(out.contains("[feedback] no-emoji"));
        assert!(out.contains("## Memory"));
    }

    #[test]
    fn test_format_index_overflow() {
        let entries: Vec<_> = (0..120)
            .map(|i| make_entry("experience", &format!("mem-{i}")))
            .collect();
        let out = format_index(&entries).unwrap();
        assert!(out.contains("... and 20 more"));
    }

    #[test]
    fn test_format_recalled_empty() {
        let result = RecallResult::default();
        assert!(format_recalled(&result).is_none());
    }

    #[test]
    fn test_format_recalled_wraps_in_system_reminder() {
        let mut result = RecallResult::default();
        result
            .pinned
            .push(make_memory("feedback", "m1", "Don't add emoji."));
        let out = format_recalled(&result).unwrap();
        assert!(out.starts_with("<system-reminder>"));
        assert!(out.ends_with("</system-reminder>"));
        assert!(out.contains("kind=\"feedback\""));
        assert!(out.contains("Don't add emoji."));
    }

    #[test]
    fn test_memory_block_format() {
        let m = make_memory("feedback", "abc-123", "  No emoji please.  ");
        let block = format_memory_block(&m);
        assert!(block.starts_with("<memory kind=\"feedback\" id=\"abc-123\">"));
        assert!(block.contains("No emoji please."));
        assert!(block.ends_with("</memory>"));
    }

    #[test]
    fn test_write_guidance_contains_key_phrases() {
        let g = format_write_guidance();
        assert!(g.contains("Do NOT save"));
        assert!(g.contains("Quality over quantity"));
    }
}
