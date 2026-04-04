// ============================================================================
// Three-Level Context Compression
//
// The context window is finite. Long conversations will fill it.
// Instead of failing when the window is full, we compress.
//
// Three levels, each more aggressive than the last:
//
//   Level 1 — Tool Result Budget
//     Problem:  A single tool result (e.g., a 50KB file) can fill 10% of context.
//     Solution: Cap each tool result at max_chars. Overflow is truncated with a
//               note: "[truncated, showing first N chars of M total]".
//     Cost:     Zero. No LLM call. Pure string truncation.
//
//   Level 2 — Context Collapse (future)
//     Problem:  Many small messages accumulate over time.
//     Solution: Fold old message spans into compressed placeholders.
//               Reversible — can expand if needed.
//     Cost:     Zero LLM calls. Rule-based folding.
//     Status:   Placeholder for future implementation.
//
//   Level 3 — Auto Compact
//     Problem:  Context at ~70% capacity. Need significant reduction.
//     Solution: Summarize all messages before the keep-window using a small,
//               cheap model. Replace them with a CompactBoundary containing
//               the summary.
//     Cost:     One LLM call (small model). Irreversible.
//
// Degradation chain: Normal → Level 1 → Level 3 → Overflow error
// (Level 2 is a future optimization slot between 1 and 3.)
// ============================================================================

use anyhow::Result;

use crate::adapters::llm::LlmProvider;
use crate::common::{ContentBlock, Message};

/// Maximum characters for a single tool result before truncation.
/// ~5000 chars ≈ ~1250 tokens. Generous enough for most results,
/// small enough to prevent a single tool from dominating context.
const TOOL_RESULT_MAX_CHARS: usize = 5_000;

/// Number of recent messages to preserve during compaction.
/// These are kept verbatim — only older messages are summarized.
/// 6 messages ≈ 3 user-assistant exchanges.
const KEEP_RECENT_MESSAGES: usize = 6;

/// System prompt for the compact model.
/// Instructs the summarizer to preserve the information the agent needs
/// to continue working without the original messages.
const COMPACT_SYSTEM_PROMPT: &str = "\
Summarize this conversation history, preserving:
- Key decisions and their reasoning
- Important data, names, and identifiers mentioned
- Current task state and next steps
- Any user preferences or corrections expressed
- Errors encountered and how they were resolved

Be concise but complete. This summary will be the ONLY context for future turns — \
anything not in the summary is permanently lost.";

// ============================================================================
// Level 1: Tool Result Budget
// ============================================================================

/// Apply tool result budgets to messages in place.
///
/// Scans all messages for ToolResult blocks exceeding the size limit.
/// Truncates oversized results and appends a truncation notice.
///
/// This is the cheapest compression — no LLM call, no data loss
/// beyond the truncated tail. Applied before every API call.
pub fn apply_tool_result_budget(messages: &mut [Message]) {
    for message in messages.iter_mut() {
        for block in message.content.iter_mut() {
            if let ContentBlock::ToolResult { content, .. } = block {
                if content.len() > TOOL_RESULT_MAX_CHARS {
                    let total_len = content.len();
                    content.truncate(TOOL_RESULT_MAX_CHARS);
                    content.push_str(&format!(
                        "\n\n[truncated, showing first {TOOL_RESULT_MAX_CHARS} chars of {total_len} total]"
                    ));
                }
            }
        }
    }
}

// ============================================================================
// Level 3: Auto Compact
// ============================================================================

/// Compact the conversation by summarizing old messages.
///
/// Called when the context manager detects Compact pressure.
/// Uses a small, cheap model to generate a summary of old messages,
/// then replaces them with a CompactBoundary.
///
/// # Arguments
/// * `messages` — the full message history (will be modified in place)
/// * `compact_model` — a cheap LLM provider (e.g., Haiku) for summarization
/// * `compact_count` — how many times we've compacted (for the boundary ID)
///
/// # Returns
/// Ok(true) if compaction happened, Ok(false) if not enough messages to compact.
pub async fn auto_compact(
    messages: &mut Vec<Message>,
    compact_model: &dyn LlmProvider,
    compact_count: u32,
) -> Result<bool> {
    // Need more than KEEP_RECENT to have something to summarize.
    if messages.len() <= KEEP_RECENT_MESSAGES {
        return Ok(false);
    }

    let split_point = messages.len() - KEEP_RECENT_MESSAGES;
    let to_summarize = &messages[..split_point];
    let to_keep = messages[split_point..].to_vec();

    // Find existing compact summary (if any) to build upon.
    let existing_summary = to_summarize
        .iter()
        .filter_map(|m| {
            m.content.iter().find_map(|b| match b {
                ContentBlock::CompactBoundary { summary } => Some(summary.as_str()),
                _ => None,
            })
        })
        .last()
        .unwrap_or("");

    // Format messages for the summarizer.
    let messages_text = format_messages_for_summary(to_summarize);
    let summarize_prompt = if existing_summary.is_empty() {
        format!("Summarize the following conversation:\n\n{messages_text}")
    } else {
        format!(
            "Previous summary:\n{existing_summary}\n\n\
             New messages to incorporate:\n\n{messages_text}"
        )
    };

    // Call the compact model.
    let summary_message = Message::user("compact-input", summarize_prompt);
    let config = crate::adapters::llm::ProviderConfig {
        model: crate::kernel::r#loop::default_model(),
        max_tokens: 4096,
        temperature: Some(0.0), // Deterministic summarization.
        ..Default::default()
    };

    // Collect the full response (non-streaming for simplicity).
    use futures::StreamExt;
    let mut stream = compact_model
        .stream(COMPACT_SYSTEM_PROMPT, &[summary_message], &[], &config)
        .await?;

    let mut summary = String::new();
    while let Some(event) = stream.next().await {
        if let Ok(crate::common::StreamEvent::TextDelta { text }) = event {
            summary.push_str(&text);
        }
    }

    if summary.is_empty() {
        anyhow::bail!("compact model returned empty summary");
    }

    // Replace old messages with the summary boundary.
    let boundary_id = format!("compact-{compact_count}");
    *messages = vec![Message::compact_boundary(boundary_id, &summary)];
    messages.extend(to_keep);

    tracing::info!(
        old_count = split_point,
        new_count = messages.len(),
        summary_len = summary.len(),
        "context compacted"
    );

    Ok(true)
}

/// Format messages into a readable text block for the summarizer.
fn format_messages_for_summary(messages: &[Message]) -> String {
    messages
        .iter()
        .filter_map(|m| {
            let role = match m.role {
                crate::common::Role::User => "User",
                crate::common::Role::Assistant => "Assistant",
                crate::common::Role::System => "System",
            };

            let text: String = m
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    ContentBlock::ToolUse { name, .. } => Some(name.as_str()),
                    ContentBlock::ToolResult { content, .. } => {
                        // Truncate large tool results in the summary input too.
                        if content.len() > 500 {
                            Some(&content[..500])
                        } else {
                            Some(content.as_str())
                        }
                    }
                    ContentBlock::CompactBoundary { summary } => Some(summary.as_str()),
                    ContentBlock::Thinking { .. } => None, // Skip thinking in summaries.
                    ContentBlock::Collapsed { .. } => None,
                })
                .collect::<Vec<&str>>()
                .join("\n");

            if text.is_empty() {
                None
            } else {
                Some(format!("{role}: {text}"))
            }
        })
        .collect::<Vec<String>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_result_budget_truncates_large_results() {
        let mut messages = vec![Message::tool_results(
            "1",
            vec![ContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: "x".repeat(10_000),
                is_error: false,
            }],
        )];

        apply_tool_result_budget(&mut messages);

        if let ContentBlock::ToolResult { content, .. } = &messages[0].content[0] {
            assert!(content.len() < 10_000);
            assert!(content.contains("[truncated"));
        } else {
            panic!("expected tool result");
        }
    }

    #[test]
    fn test_tool_result_budget_preserves_small_results() {
        let original = "small result";
        let mut messages = vec![Message::tool_results(
            "1",
            vec![ContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: original.into(),
                is_error: false,
            }],
        )];

        apply_tool_result_budget(&mut messages);

        if let ContentBlock::ToolResult { content, .. } = &messages[0].content[0] {
            assert_eq!(content, original);
        }
    }

    #[test]
    fn test_format_messages_for_summary() {
        let messages = vec![
            Message::user("1", "What is Rust?"),
            Message::assistant(
                "2",
                vec![ContentBlock::Text {
                    text: "Rust is a systems programming language.".into(),
                }],
            ),
        ];

        let result = format_messages_for_summary(&messages);
        assert!(result.contains("User: What is Rust?"));
        assert!(result.contains("Assistant: Rust is a systems programming language."));
    }
}
