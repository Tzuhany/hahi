// ============================================================================
// Token Usage Tracking
//
// Tracks token consumption across LLM API calls.
// Used for billing, monitoring, and context pressure detection.
// ============================================================================

use serde::{Deserialize, Serialize};

/// Token usage statistics for a single LLM API call.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
}

impl TokenUsage {
    /// Total tokens consumed (input + output, excluding cache).
    pub fn total(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }

    /// Billed input tokens: input that was NOT served from cache.
    ///
    /// Cache hits are charged at a discounted rate — this gives the
    /// count that maps to full-price input billing.
    #[allow(dead_code)]
    pub fn billed_input(&self) -> u64 {
        self.input_tokens.saturating_sub(self.cache_read_tokens)
    }

    /// True input cost including cache creation overhead.
    ///
    /// Cache creation costs extra on first write, then pays off on subsequent hits.
    #[allow(dead_code)]
    pub fn total_with_cache_overhead(&self) -> u64 {
        self.input_tokens + self.output_tokens + self.cache_creation_tokens
    }

    /// Accumulate usage from another call into this one.
    pub fn accumulate(&mut self, other: &TokenUsage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_creation_tokens += other.cache_creation_tokens;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_usage_accumulate() {
        let mut total = TokenUsage::default();
        let call1 = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            ..Default::default()
        };
        let call2 = TokenUsage {
            input_tokens: 200,
            output_tokens: 80,
            cache_read_tokens: 90,
            ..Default::default()
        };

        total.accumulate(&call1);
        total.accumulate(&call2);

        assert_eq!(total.input_tokens, 300);
        assert_eq!(total.output_tokens, 130);
        assert_eq!(total.cache_read_tokens, 90);
        assert_eq!(total.total(), 430);
    }
}
