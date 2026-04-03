// ============================================================================
// Context Collapse — L2 Compression
//
// Sits between L1 (tool result truncation) and L3 (LLM summarization).
// Folds old message spans into lightweight placeholders WITHOUT calling the LLM.
//
// Why a separate level?
//   L1 handles individual oversized results.
//   L3 is expensive (requires an LLM call).
//   L2 fills the gap: when context is growing but not critical,
//   fold old segments cheaply. If L2 isn't enough, L3 takes over.
//
// Key property: REVERSIBLE.
//   Original messages stay in PG. The Collapsed placeholder records
//   the range of message IDs it replaced. The context manager can
//   "expand" a collapsed segment if needed (though it rarely does).
//
// Trigger: context usage 50-70% (below L3's 70% threshold).
// ============================================================================

use crate::common::{ContentBlock, Message, Role};

/// A collapsed segment — replaces a span of messages in the context.
///
/// Not a separate type; it's a `ContentBlock::Collapsed` variant.
/// This module adds the variant and provides collapse/expand operations.

/// Number of recent messages to never collapse.
/// These stay verbatim — only older messages get folded.
const KEEP_RECENT: usize = 10;

/// Minimum messages in a segment worth collapsing.
/// Collapsing 2-3 messages saves almost nothing.
const MIN_SEGMENT_SIZE: usize = 6;

/// Result of a collapse operation.
pub struct CollapseResult {
    /// The new message list with collapsed segments.
    pub messages: Vec<Message>,
    /// How many messages were collapsed.
    pub collapsed_count: usize,
}

/// Attempt to collapse old messages to reduce context size.
///
/// Strategy:
///   1. Keep the last KEEP_RECENT messages untouched
///   2. Take all earlier messages as one segment
///   3. If the segment is large enough, fold it into a Collapsed block
///   4. Build a short summary from role counts (no LLM call)
///
/// Returns None if there's nothing worth collapsing.
pub fn collapse(messages: &[Message]) -> Option<CollapseResult> {
    if messages.len() <= KEEP_RECENT + MIN_SEGMENT_SIZE {
        return None;
    }

    let split = messages.len() - KEEP_RECENT;
    let to_collapse = &messages[..split];
    let to_keep = &messages[split..];

    // Build a rule-based summary (no LLM needed).
    let summary = build_collapse_summary(to_collapse);

    // Record the range for potential expansion.
    let first_id = to_collapse
        .first()
        .map(|m| m.id.clone())
        .unwrap_or_default();
    let last_id = to_collapse.last().map(|m| m.id.clone()).unwrap_or_default();

    let placeholder = Message {
        id: format!("collapsed-{first_id}-{last_id}"),
        role: Role::System,
        content: vec![ContentBlock::Collapsed {
            folded_count: to_collapse.len() as u32,
            first_message_id: first_id,
            last_message_id: last_id,
            summary,
        }],
    };

    let mut result = vec![placeholder];
    result.extend_from_slice(to_keep);

    Some(CollapseResult {
        collapsed_count: to_collapse.len(),
        messages: result,
    })
}

/// Build a summary string from message counts — pure rule-based, no LLM.
///
/// Example: "12 messages collapsed (5 user, 6 assistant, 1 system)"
fn build_collapse_summary(messages: &[Message]) -> String {
    let mut user_count = 0u32;
    let mut assistant_count = 0u32;
    let mut system_count = 0u32;
    let mut tool_count = 0u32;

    for msg in messages {
        match msg.role {
            Role::User => user_count += 1,
            Role::Assistant => assistant_count += 1,
            Role::System => system_count += 1,
        }
        for block in &msg.content {
            if matches!(block, ContentBlock::ToolUse { .. }) {
                tool_count += 1;
            }
        }
    }

    let total = messages.len();
    let mut parts = vec![format!("{total} messages collapsed")];

    let mut breakdown = Vec::new();
    if user_count > 0 {
        breakdown.push(format!("{user_count} user"));
    }
    if assistant_count > 0 {
        breakdown.push(format!("{assistant_count} assistant"));
    }
    if tool_count > 0 {
        breakdown.push(format!("{tool_count} tool calls"));
    }
    if system_count > 0 {
        breakdown.push(format!("{system_count} system"));
    }

    if !breakdown.is_empty() {
        parts.push(format!("({})", breakdown.join(", ")));
    }

    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_messages(count: usize) -> Vec<Message> {
        (0..count)
            .map(|i| {
                if i % 2 == 0 {
                    Message::user(format!("m{i}"), format!("message {i}"))
                } else {
                    Message::assistant(
                        format!("m{i}"),
                        vec![ContentBlock::Text {
                            text: format!("reply {i}"),
                        }],
                    )
                }
            })
            .collect()
    }

    #[test]
    fn test_collapse_too_few_messages() {
        let msgs = make_messages(12); // 12 < KEEP_RECENT + MIN_SEGMENT_SIZE
        assert!(collapse(&msgs).is_none());
    }

    #[test]
    fn test_collapse_enough_messages() {
        let msgs = make_messages(30);
        let result = collapse(&msgs).unwrap();

        // Should have 1 collapsed + KEEP_RECENT kept.
        assert_eq!(result.messages.len(), KEEP_RECENT + 1);
        assert_eq!(result.collapsed_count, 20); // 30 - 10

        // First message should be the collapsed placeholder.
        assert!(matches!(
            &result.messages[0].content[0],
            ContentBlock::Collapsed {
                folded_count: 20,
                ..
            }
        ));
    }

    #[test]
    fn test_collapse_summary_format() {
        let msgs = make_messages(10);
        let summary = build_collapse_summary(&msgs);
        assert!(summary.contains("10 messages collapsed"));
        assert!(summary.contains("user"));
        assert!(summary.contains("assistant"));
    }
}
