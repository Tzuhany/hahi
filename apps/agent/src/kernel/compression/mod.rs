// ============================================================================
// Compression Pipeline
//
// Wraps the three-level compression strategy in a composable, testable form.
// Each stage is an independent unit that decides whether to apply itself.
//
// Stages in order:
//   L1Budget  — always runs, silently truncates oversized tool results
//   L2Collapse — runs when pressure >= Compact; folds old messages (no LLM)
//   L3Compact  — runs only if L2 wasn't enough; LLM summarization (expensive)
//
// The pipeline stops after the first pressure-reducing stage succeeds.
// L1 never counts as "pressure reduced" — it runs unconditionally as prep.
//
// Usage:
//   let pipeline = CompressionPipeline::standard(provider, compact_provider);
//
//   // Before every LLM call:
//   pipeline.apply_budget(messages);
//
//   // When pressure is detected:
//   let events = pipeline.apply_pressure(messages, pressure, &mut compact_count).await;
//   for event in events { emit(event.into_loop_event()); }
// ============================================================================

pub mod collapse;
pub mod compact;

use std::sync::Arc;

use async_trait::async_trait;

use crate::adapters::llm::LlmProvider;
use crate::common::Message;
use crate::kernel::compression::compact::{apply_tool_result_budget, auto_compact};
use crate::kernel::context::{ContextPressure, estimate_tokens};

// ============================================================================
// Stage trait
// ============================================================================

/// Context passed to every compression stage.
pub struct PipelineCx<'a> {
    pub messages: &'a mut Vec<Message>,
    pub pressure: ContextPressure,
    /// Incremented each time L3 compacts. Used for boundary IDs.
    pub compact_count: &'a mut u32,
}

/// An event produced by a pressure-reducing stage.
#[derive(Debug, Clone)]
pub enum StageEvent {
    /// L2 folded old messages into a placeholder. Reversible.
    Collapsed { folded_count: usize },
    /// L3 replaced old messages with an LLM summary. Irreversible.
    Compacted { pre_tokens: usize },
}

/// A single stage in the compression pipeline.
///
/// Each stage is self-contained: it receives the full pipeline context and
/// decides independently whether to apply itself.
/// Returns `Some(StageEvent)` if it made a change, `None` to pass through.
#[async_trait]
pub trait CompressionStage: Send + Sync {
    fn name(&self) -> &'static str;
    async fn apply(&self, cx: &mut PipelineCx<'_>) -> Option<StageEvent>;
}

// ============================================================================
// Built-in stages
// ============================================================================

/// L1: Tool result budget — silently truncates oversized tool results.
///
/// Always runs. Zero cost, zero LLM calls. Applied before every API call
/// regardless of pressure level.
pub struct L1Budget;

/// L2: Context collapse — folds old messages into placeholders.
///
/// Runs when pressure >= Compact. No LLM call. Reversible.
pub struct L2Collapse;

/// L3: Auto compact — summarizes old messages with a cheap LLM.
///
/// Runs only when L2 couldn't reduce pressure enough. Irreversible.
pub struct L3Compact {
    provider: Arc<dyn LlmProvider>,
}

impl L3Compact {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl CompressionStage for L1Budget {
    fn name(&self) -> &'static str {
        "tool-result-budget"
    }

    async fn apply(&self, cx: &mut PipelineCx<'_>) -> Option<StageEvent> {
        apply_tool_result_budget(cx.messages);
        None // L1 runs silently — no event, doesn't stop the pipeline
    }
}

#[async_trait]
impl CompressionStage for L2Collapse {
    fn name(&self) -> &'static str {
        "context-collapse"
    }

    async fn apply(&self, cx: &mut PipelineCx<'_>) -> Option<StageEvent> {
        if cx.pressure < ContextPressure::Compact {
            return None;
        }
        let api_messages = cx.messages.clone(); // collapse works on a slice
        if let Some(result) = collapse::collapse(&api_messages) {
            *cx.messages = result.messages;
            tracing::debug!(folded = result.collapsed_count, "L2 collapse applied");
            Some(StageEvent::Collapsed {
                folded_count: result.collapsed_count,
            })
        } else {
            None
        }
    }
}

#[async_trait]
impl CompressionStage for L3Compact {
    fn name(&self) -> &'static str {
        "llm-summarize"
    }

    async fn apply(&self, cx: &mut PipelineCx<'_>) -> Option<StageEvent> {
        if cx.pressure < ContextPressure::Compact {
            return None;
        }
        // Snapshot token count before compaction for the event.
        let pre_tokens = estimate_tokens(cx.messages);

        match auto_compact(cx.messages, self.provider.as_ref(), *cx.compact_count).await {
            Ok(true) => {
                *cx.compact_count += 1;
                tracing::debug!(
                    pre_tokens,
                    compact_count = *cx.compact_count,
                    "L3 compact applied"
                );
                Some(StageEvent::Compacted { pre_tokens })
            }
            Ok(false) => None,
            Err(e) => {
                tracing::warn!(error = %e, "L3 compact failed, skipping");
                None
            }
        }
    }
}

// ============================================================================
// Pipeline
// ============================================================================

/// The assembled compression pipeline.
///
/// Holds an ordered list of stages. Stages run in order; the pipeline stops
/// after the first pressure-reducing stage (one that returns Some(StageEvent)).
/// L1Budget is exempt from this rule — it runs unconditionally and never stops
/// the chain because it returns None.
pub struct CompressionPipeline {
    stages: Vec<Box<dyn CompressionStage>>,
}

impl CompressionPipeline {
    /// Build with explicit stages (for testing or custom configurations).
    pub fn new(stages: Vec<Box<dyn CompressionStage>>) -> Self {
        Self { stages }
    }

    /// Standard three-level pipeline.
    ///
    /// * `main_provider`    — used for L3 if no compact provider is given
    /// * `compact_provider` — cheaper model for L3 summarization (optional)
    pub fn standard(
        main_provider: Arc<dyn LlmProvider>,
        compact_provider: Option<Arc<dyn LlmProvider>>,
    ) -> Self {
        let l3_provider = compact_provider.unwrap_or_else(|| Arc::clone(&main_provider));
        Self::new(vec![
            Box::new(L1Budget),
            Box::new(L2Collapse),
            Box::new(L3Compact::new(l3_provider)),
        ])
    }

    /// Apply L1 budget only — used before every API call regardless of pressure.
    pub fn apply_budget(&self, messages: &mut Vec<Message>) {
        apply_tool_result_budget(messages);
    }

    /// Run pressure-reducing stages (L2, L3) against the current context.
    ///
    /// Stages run in order. The pipeline stops after the first stage that
    /// returns a `StageEvent`, so L2 is tried before L3.
    ///
    /// Returns the list of events that fired (0–1 in normal operation).
    pub async fn apply_pressure(
        &self,
        messages: &mut Vec<Message>,
        pressure: ContextPressure,
        compact_count: &mut u32,
    ) -> Vec<StageEvent> {
        let mut events = Vec::new();
        let mut cx = PipelineCx {
            messages,
            pressure,
            compact_count,
        };

        for stage in &self.stages {
            if let Some(event) = stage.apply(&mut cx).await {
                tracing::debug!(stage = stage.name(), "compression stage fired");
                events.push(event);
                // Stop after first pressure-reducing stage succeeds.
                break;
            }
        }

        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{ContentBlock, Message};

    fn messages_with_tool_result(content: String) -> Vec<Message> {
        vec![Message::tool_results(
            "1",
            vec![ContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content,
                is_error: false,
            }],
        )]
    }

    #[tokio::test]
    async fn test_l1_budget_truncates_via_pipeline() {
        let big = "x".repeat(10_000);
        let mut messages = messages_with_tool_result(big);
        apply_tool_result_budget(&mut messages);
        if let ContentBlock::ToolResult { content, .. } = &messages[0].content[0] {
            assert!(content.contains("[truncated"));
        }
    }

    #[tokio::test]
    async fn test_l2_skips_on_normal_pressure() {
        let stage = L2Collapse;
        let mut messages = vec![Message::user("1", "hello")];
        let mut compact_count = 0u32;
        let result = stage
            .apply(&mut PipelineCx {
                messages: &mut messages,
                pressure: ContextPressure::Normal,
                compact_count: &mut compact_count,
            })
            .await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_l2_skips_when_too_few_messages() {
        let stage = L2Collapse;
        let mut messages: Vec<Message> =
            (0..5).map(|i| Message::user(i.to_string(), "x")).collect();
        let mut compact_count = 0u32;
        let result = stage
            .apply(&mut PipelineCx {
                messages: &mut messages,
                pressure: ContextPressure::Compact,
                compact_count: &mut compact_count,
            })
            .await;
        // Only 5 messages — not enough to collapse (MIN_SEGMENT_SIZE check in collapse module).
        assert!(result.is_none());
    }
}
