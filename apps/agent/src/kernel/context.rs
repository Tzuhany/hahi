// ============================================================================
// Context Window Manager
//
// Tracks how much of the LLM's context window is used and signals
// when compression is needed. The agent loop checks this before every
// API call — if pressure is high, it triggers compaction before proceeding.
//
// The context window is a fixed resource (e.g., 200k tokens).
// As the conversation grows, we approach the limit. Without intervention,
// the API returns a 413 (prompt too long) and the turn fails.
//
// This module prevents that by detecting pressure early and triggering
// the three-level compression pipeline (see compact.rs).
//
// Token estimation:
//   We don't call a tokenizer (too slow for every turn). Instead, we use
//   a character-based heuristic: ~4 chars per token for English.
//   This is imprecise but sufficient for pressure detection — we only
//   need to know "are we close to the limit?", not the exact count.
// ============================================================================

use crate::common::Message;

/// Approximate characters per token.
/// English averages ~4 chars/token. CJK is closer to 1.5-2.
/// We use a conservative estimate — better to compact too early than too late.
const CHARS_PER_TOKEN: usize = 4;

/// What the context manager recommends the agent loop do.
///
/// Variants are ordered from least to most severe, so `>=` comparisons work:
///   `pressure >= ContextPressure::Compact` means "compact OR overflow".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContextPressure {
    /// Plenty of room. Continue normally.
    Normal,

    /// Approaching the limit. Trigger compaction before the next API call.
    Compact,

    /// Already over the limit even after compaction. Abort the turn.
    Overflow,
}

/// Context window manager.
///
/// Configured once per session with the model's context window size.
/// Called before every API request to check pressure.
pub struct ContextManager {
    /// Maximum context window in tokens.
    max_tokens: usize,

    /// Trigger compaction when usage exceeds this fraction of max_tokens.
    /// Default: 0.7 (70%). Leaves headroom for the model's response.
    compact_threshold: f64,
}

impl ContextManager {
    /// Create a new context manager for a given model's context window.
    pub fn new(max_tokens: usize) -> Self {
        Self {
            max_tokens,
            compact_threshold: 0.7,
        }
    }

    /// Override the compact threshold (fraction of max_tokens).
    /// Must be between 0.0 and 1.0.
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.compact_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Check context pressure based on the current message history.
    ///
    /// Called by the agent loop before every LLM API call.
    /// The loop reacts accordingly:
    ///   Normal   → proceed with the API call
    ///   Compact  → run compact pipeline first, then proceed
    ///   Overflow → abort the turn with an error
    pub fn check_pressure(&self, messages: &[Message]) -> ContextPressure {
        let estimated_tokens = estimate_tokens(messages);
        let threshold = (self.max_tokens as f64 * self.compact_threshold) as usize;

        if estimated_tokens > self.max_tokens {
            ContextPressure::Overflow
        } else if estimated_tokens > threshold {
            ContextPressure::Compact
        } else {
            ContextPressure::Normal
        }
    }

    /// Return messages after the last compact boundary.
    ///
    /// Only these messages are sent to the LLM API — everything before
    /// the boundary has been summarized into the boundary's content.
    pub fn messages_for_api<'a>(&self, messages: &'a [Message]) -> &'a [Message] {
        let boundary_pos = messages.iter().rposition(|m| m.is_compact_boundary());

        match boundary_pos {
            Some(pos) => &messages[pos..],
            None => messages,
        }
    }

    /// Remaining tokens before the compact threshold.
    pub fn remaining_before_compact(&self, messages: &[Message]) -> usize {
        let used = estimate_tokens(messages);
        let threshold = (self.max_tokens as f64 * self.compact_threshold) as usize;
        threshold.saturating_sub(used)
    }
}

/// Estimate the token count of a message list.
///
/// Uses character count / CHARS_PER_TOKEN as a heuristic.
/// Not exact, but good enough for pressure detection.
/// Exposed for use by the compression pipeline stages.
pub fn estimate_tokens(messages: &[Message]) -> usize {
    let total_chars: usize = messages
        .iter()
        .map(|m| {
            m.content
                .iter()
                .map(|block| block_char_count(block))
                .sum::<usize>()
        })
        .sum();

    total_chars / CHARS_PER_TOKEN
}

/// Character count of a single content block.
fn block_char_count(block: &crate::common::ContentBlock) -> usize {
    use crate::common::ContentBlock;
    match block {
        ContentBlock::Text { text } => text.len(),
        ContentBlock::Thinking { text } => text.len(),
        ContentBlock::ToolUse { input, name, .. } => name.len() + input.to_string().len(),
        ContentBlock::ToolResult { content, .. } => content.len(),
        ContentBlock::CompactBoundary { summary } => summary.len(),
        ContentBlock::Collapsed { summary, .. } => summary.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Message;

    #[test]
    fn test_normal_pressure() {
        let manager = ContextManager::new(200_000);
        let messages = vec![Message::user("1", "hello")];
        assert_eq!(manager.check_pressure(&messages), ContextPressure::Normal);
    }

    #[test]
    fn test_compact_pressure() {
        let manager = ContextManager::new(100); // Very small window.
        // 70% of 100 tokens = 70 tokens = ~280 chars.
        let long_text = "x".repeat(300);
        let messages = vec![Message::user("1", long_text)];
        assert_eq!(manager.check_pressure(&messages), ContextPressure::Compact);
    }

    #[test]
    fn test_overflow_pressure() {
        let manager = ContextManager::new(10); // Tiny window.
        let messages = vec![Message::user("1", "x".repeat(100))];
        assert_eq!(manager.check_pressure(&messages), ContextPressure::Overflow);
    }

    #[test]
    fn test_messages_for_api_with_boundary() {
        let manager = ContextManager::new(200_000);
        let messages = vec![
            Message::user("1", "old message"),
            Message::compact_boundary("2", "summary of prior conversation"),
            Message::user("3", "new message"),
        ];

        let api_messages = manager.messages_for_api(&messages);
        assert_eq!(api_messages.len(), 2); // boundary + new message
    }

    #[test]
    fn test_messages_for_api_without_boundary() {
        let manager = ContextManager::new(200_000);
        let messages = vec![Message::user("1", "first"), Message::user("2", "second")];

        let api_messages = manager.messages_for_api(&messages);
        assert_eq!(api_messages.len(), 2); // All messages.
    }
}
