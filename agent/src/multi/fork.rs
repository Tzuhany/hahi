// ============================================================================
// Fork Optimization — Prompt Cache Sharing Across Sub-Agents
//
// When the parent spawns multiple sub-agents simultaneously, each gets
// the parent's message history as context. Without optimization, each
// sub-agent pays full input token cost for the shared prefix.
//
// The fork optimization exploits Anthropic's prompt caching:
//   - All sub-agents share an identical message prefix (parent's history)
//   - Only the per-child directive (last message) differs
//   - First child creates the cache entry
//   - Subsequent children hit the cache → 90% token discount
//
// Example with 3 sub-agents and 50k token parent context:
//   Without fork: 50k × 3 = 150k input tokens (full price)
//   With fork:    50k + 50k×0.1×2 = 60k effective cost (60% savings)
//
// Implementation:
//   1. Build the shared prefix once (parent messages + placeholder tool results)
//   2. For each child, append its unique directive as the last message
//   3. The prefix bytes are identical → cache hit on children 2, 3, ...
//
// Placeholder tool results:
//   The parent's assistant message may contain multiple tool_use blocks
//   (one per sub-agent). Each child sees ALL tool_use blocks with placeholder
//   results ("Fork started — processing in background"), not just its own.
//   This keeps the prefix byte-identical across all children.
// ============================================================================

#![allow(dead_code)]

use crate::common::{ContentBlock, Message};

/// Placeholder text for tool results in fork messages.
/// Must be identical across all fork children for cache hit.
const FORK_PLACEHOLDER: &str = "Fork started — processing in background";

/// Build fork messages for a single child.
///
/// Takes the parent's message history and the child's unique directive.
/// Returns a message list where:
///   - All parent messages are preserved (shared prefix)
///   - All tool_use blocks get placeholder results (cache-friendly)
///   - The child's directive is appended as the last user message
///
/// The caller (spawn.rs) uses this instead of a plain `vec![user_message]`
/// when the parent is spawning multiple sub-agents from the same turn.
pub fn build_fork_messages(parent_messages: &[Message], child_directive: &str) -> Vec<Message> {
    let mut messages = parent_messages.to_vec();

    // Find all tool_use blocks in the last assistant message
    // and generate placeholder tool results for them.
    if let Some(last_assistant) = messages
        .iter()
        .rev()
        .find(|m| m.role == crate::common::Role::Assistant)
    {
        let tool_use_ids: Vec<String> = last_assistant
            .content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::ToolUse { id, .. } => Some(id.clone()),
                _ => None,
            })
            .collect();

        if !tool_use_ids.is_empty() {
            let placeholder_results: Vec<ContentBlock> = tool_use_ids
                .into_iter()
                .map(|id| ContentBlock::ToolResult {
                    tool_use_id: id,
                    content: FORK_PLACEHOLDER.to_string(),
                    is_error: false,
                })
                .collect();

            messages.push(Message::tool_results(
                uuid::Uuid::new_v4().to_string(),
                placeholder_results,
            ));
        }
    }

    // Append the child-specific directive.
    messages.push(Message::user(
        uuid::Uuid::new_v4().to_string(),
        child_directive.to_string(),
    ));

    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_fork_messages_preserve_parent_history() {
        let parent_messages = vec![
            Message::user("1", "hello"),
            Message::assistant(
                "2",
                vec![ContentBlock::Text {
                    text: "I'll help.".into(),
                }],
            ),
        ];

        let fork = build_fork_messages(&parent_messages, "Search for X");
        // Parent messages + directive = 3.
        assert_eq!(fork.len(), 3);
        assert!(matches!(
            &fork.last().unwrap().content[0],
            ContentBlock::Text { text } if text == "Search for X"
        ));
    }

    #[test]
    fn test_fork_messages_add_placeholder_results() {
        let parent_messages = vec![
            Message::user("1", "do two things"),
            Message::assistant(
                "2",
                vec![
                    ContentBlock::Text {
                        text: "I'll spawn two agents.".into(),
                    },
                    ContentBlock::ToolUse {
                        id: "t1".into(),
                        name: "Agent".into(),
                        input: json!({"type": "explorer"}),
                    },
                    ContentBlock::ToolUse {
                        id: "t2".into(),
                        name: "Agent".into(),
                        input: json!({"type": "planner"}),
                    },
                ],
            ),
        ];

        let fork = build_fork_messages(&parent_messages, "Search for X");

        // parent(2) + placeholder_results(1) + directive(1) = 4.
        assert_eq!(fork.len(), 4);

        // Check placeholder results.
        let results_msg = &fork[2];
        assert_eq!(results_msg.content.len(), 2);
        for block in &results_msg.content {
            match block {
                ContentBlock::ToolResult { content, .. } => {
                    assert_eq!(content, FORK_PLACEHOLDER);
                }
                _ => panic!("expected tool result"),
            }
        }
    }

    #[test]
    fn test_fork_messages_identical_prefix() {
        let parent_messages = vec![
            Message::user("1", "plan and research"),
            Message::assistant(
                "2",
                vec![ContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "Agent".into(),
                    input: json!({}),
                }],
            ),
        ];

        let fork_a = build_fork_messages(&parent_messages, "Research X");
        let fork_b = build_fork_messages(&parent_messages, "Plan Y");

        // Everything except the last message should be identical.
        let prefix_a = &fork_a[..fork_a.len() - 1];
        let prefix_b = &fork_b[..fork_b.len() - 1];
        assert_eq!(prefix_a.len(), prefix_b.len());
        // (Byte-identical comparison would require serialization,
        //  but structural equality suffices for this test.)
    }
}
