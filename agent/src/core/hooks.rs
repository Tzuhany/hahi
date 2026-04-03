// ============================================================================
// Stop Hooks — User-Defined Validation Before Turn Completion
//
// Inspired by Claude Code's stop hooks. When the LLM finishes its response
// (no more tool_use), hooks run BEFORE the turn is considered complete.
//
// If any hook blocks → the error is injected into the conversation →
// the LLM sees it and self-corrects → hooks run again.
//
// This creates a feedback loop:
//   LLM responds → hooks check → fail? → LLM sees error → retries → hooks check → pass
//
// Three hook points:
//   PreToolUse   — before a tool executes (can block dangerous calls)
//   PostToolUse  — after a tool executes (can audit results)
//   PreComplete  — before the turn ends (can enforce output requirements)
//
// Hooks are NOT shell scripts (we're cloud, no user shell).
// They're configurable rules — either built-in or user-defined via instructions.
// ============================================================================

use async_trait::async_trait;
use serde_json::Value;

use crate::common::ToolOutput;

/// The event a hook evaluates.
#[derive(Debug, Clone)]
pub enum HookEvent {
    /// A tool is about to be executed.
    PreToolUse { tool_name: String, input: Value },

    /// A tool has finished executing.
    PostToolUse {
        tool_name: String,
        input: Value,
        output: ToolOutput,
    },

    /// The LLM has finished responding (no tool_use). Turn is about to end.
    PreComplete { assistant_text: String },
}

/// A hook's decision.
#[derive(Debug, Clone)]
pub enum HookDecision {
    /// Allow the operation to proceed.
    Allow,

    /// Block the operation. The reason is injected into the conversation
    /// so the LLM can see it and self-correct.
    Block { reason: String },
}

/// Trait for hook implementations.
///
/// Hooks are stateless evaluators. They receive an event, return a decision.
/// Multiple hooks can be registered; ALL must Allow for the operation to proceed.
#[async_trait]
pub trait Hook: Send + Sync {
    /// Human-readable name for logging.
    fn name(&self) -> &str;

    /// Which event type this hook handles.
    fn handles(&self, event: &HookEvent) -> bool;

    /// Evaluate the event. Called only if `handles()` returns true.
    async fn evaluate(&self, event: &HookEvent) -> HookDecision;
}

/// Extension trait that adds `boxed()` to any `Hook`.
pub trait HookExt: Hook + Sized + 'static {
    /// Box this hook for use in a `Vec<Box<dyn Hook>>`.
    fn boxed(self) -> Box<dyn Hook> {
        Box::new(self)
    }

    /// Require both hooks to allow the event.
    fn and<H: Hook + 'static>(self, other: H) -> AndHook<Self, H> {
        AndHook {
            left: self,
            right: other,
        }
    }

    /// Allow the event if either hook allows it.
    fn or<H: Hook + 'static>(self, other: H) -> OrHook<Self, H> {
        OrHook {
            left: self,
            right: other,
        }
    }
}

/// Blanket impl: every sized Hook automatically gets HookExt.
impl<T: Hook + Sized + 'static> HookExt for T {}

/// Hook combinator that requires both hooks to allow an event.
pub struct AndHook<A, B> {
    left: A,
    right: B,
}

/// Hook combinator that allows an event if either hook allows it.
pub struct OrHook<A, B> {
    left: A,
    right: B,
}

#[async_trait]
impl<A: Hook, B: Hook> Hook for AndHook<A, B> {
    fn name(&self) -> &str {
        "and"
    }

    fn handles(&self, event: &HookEvent) -> bool {
        self.left.handles(event) || self.right.handles(event)
    }

    async fn evaluate(&self, event: &HookEvent) -> HookDecision {
        let left = if self.left.handles(event) {
            self.left.evaluate(event).await
        } else {
            HookDecision::Allow
        };
        if matches!(left, HookDecision::Block { .. }) {
            return left;
        }

        if self.right.handles(event) {
            self.right.evaluate(event).await
        } else {
            HookDecision::Allow
        }
    }
}

#[async_trait]
impl<A: Hook, B: Hook> Hook for OrHook<A, B> {
    fn name(&self) -> &str {
        "or"
    }

    fn handles(&self, event: &HookEvent) -> bool {
        self.left.handles(event) || self.right.handles(event)
    }

    async fn evaluate(&self, event: &HookEvent) -> HookDecision {
        let left = if self.left.handles(event) {
            self.left.evaluate(event).await
        } else {
            HookDecision::Block {
                reason: "left hook did not handle event".into(),
            }
        };
        if matches!(left, HookDecision::Allow) {
            return HookDecision::Allow;
        }

        let right = if self.right.handles(event) {
            self.right.evaluate(event).await
        } else {
            HookDecision::Block {
                reason: "right hook did not handle event".into(),
            }
        };
        if matches!(right, HookDecision::Allow) {
            HookDecision::Allow
        } else {
            left
        }
    }
}

/// A collection of hooks. Evaluates all matching hooks for an event.
pub struct HookRunner {
    hooks: Vec<Box<dyn Hook>>,
}

impl HookRunner {
    pub fn new(hooks: Vec<Box<dyn Hook>>) -> Self {
        Self { hooks }
    }

    /// No hooks registered.
    pub fn empty() -> Self {
        Self { hooks: Vec::new() }
    }

    /// Run all matching hooks for an event.
    ///
    /// Returns the first Block decision, or Allow if all pass.
    /// Short-circuits on first Block — no need to run remaining hooks.
    pub async fn run(&self, event: &HookEvent) -> HookDecision {
        for hook in &self.hooks {
            if !hook.handles(event) {
                continue;
            }

            let decision = hook.evaluate(event).await;

            if let HookDecision::Block { ref reason } = decision {
                tracing::info!(
                    hook = hook.name(),
                    reason = reason.as_str(),
                    "hook blocked operation"
                );
                return decision;
            }

            // Log hook events for observability.
            match event {
                HookEvent::PostToolUse {
                    tool_name,
                    input,
                    output,
                } => {
                    tracing::debug!(
                        hook = hook.name(),
                        tool_name,
                        is_error = output.is_error,
                        input_keys = input.as_object().map(|o| o.len()).unwrap_or(0),
                        "post_tool_use hook evaluated"
                    );
                }
                HookEvent::PreComplete { assistant_text } => {
                    tracing::debug!(
                        hook = hook.name(),
                        text_len = assistant_text.len(),
                        "pre_complete hook evaluated"
                    );
                }
                _ => {}
            }
        }

        HookDecision::Allow
    }

    /// Whether any hooks are registered for PreComplete events.
    pub fn has_pre_complete_hooks(&self) -> bool {
        let test_event = HookEvent::PreComplete {
            assistant_text: String::new(),
        };
        self.hooks.iter().any(|h| h.handles(&test_event))
    }
}

// ============================================================================
// Built-in Hooks
// ============================================================================

/// Hook that prevents specific tools from being called.
/// Configured via user instructions: "Never use the DeleteDatabase tool."
pub struct ToolBlocklistHook {
    blocked_tools: Vec<String>,
}

impl ToolBlocklistHook {
    pub fn new(blocked_tools: Vec<String>) -> Self {
        Self { blocked_tools }
    }
}

#[async_trait]
impl Hook for ToolBlocklistHook {
    fn name(&self) -> &str {
        "tool_blocklist"
    }

    fn handles(&self, event: &HookEvent) -> bool {
        matches!(event, HookEvent::PreToolUse { .. })
    }

    async fn evaluate(&self, event: &HookEvent) -> HookDecision {
        if let HookEvent::PreToolUse { tool_name, input } = event {
            if self
                .blocked_tools
                .iter()
                .any(|b| b.eq_ignore_ascii_case(tool_name))
            {
                tracing::debug!(
                    tool_name,
                    input_keys = input.as_object().map(|o| o.len()).unwrap_or(0),
                    "tool blocked by blocklist"
                );
                return HookDecision::Block {
                    reason: format!(
                        "Tool '{tool_name}' is not allowed. Choose a different approach."
                    ),
                };
            }
        }
        HookDecision::Allow
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_empty_runner_always_allows() {
        let runner = HookRunner::empty();
        let event = HookEvent::PreComplete {
            assistant_text: "hello".into(),
        };
        assert!(matches!(runner.run(&event).await, HookDecision::Allow));
    }

    #[tokio::test]
    async fn test_tool_blocklist_blocks() {
        let hook = ToolBlocklistHook::new(vec!["DeleteDatabase".into()]);
        let runner = HookRunner::new(vec![Box::new(hook)]);

        let blocked = HookEvent::PreToolUse {
            tool_name: "DeleteDatabase".into(),
            input: serde_json::json!({}),
        };
        assert!(matches!(
            runner.run(&blocked).await,
            HookDecision::Block { .. }
        ));

        let allowed = HookEvent::PreToolUse {
            tool_name: "WebSearch".into(),
            input: serde_json::json!({}),
        };
        assert!(matches!(runner.run(&allowed).await, HookDecision::Allow));
    }

    #[test]
    fn test_has_pre_complete_hooks_empty() {
        let runner = HookRunner::empty();
        assert!(!runner.has_pre_complete_hooks());
    }

    // ── Combinator tests ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_and_hook_both_allow() {
        let hook = ToolBlocklistHook::new(vec!["rm".into()])
            .and(ToolBlocklistHook::new(vec!["sudo".into()]));
        let event = HookEvent::PreToolUse {
            tool_name: "WebSearch".into(),
            input: serde_json::json!({}),
        };
        assert!(matches!(hook.evaluate(&event).await, HookDecision::Allow));
    }

    #[tokio::test]
    async fn test_and_hook_first_blocks() {
        let hook = ToolBlocklistHook::new(vec!["rm".into()])
            .and(ToolBlocklistHook::new(vec!["sudo".into()]));
        let event = HookEvent::PreToolUse {
            tool_name: "rm".into(),
            input: serde_json::json!({}),
        };
        assert!(matches!(
            hook.evaluate(&event).await,
            HookDecision::Block { .. }
        ));
    }

    #[tokio::test]
    async fn test_and_hook_second_blocks() {
        let hook = ToolBlocklistHook::new(vec!["rm".into()])
            .and(ToolBlocklistHook::new(vec!["sudo".into()]));
        let event = HookEvent::PreToolUse {
            tool_name: "sudo".into(),
            input: serde_json::json!({}),
        };
        assert!(matches!(
            hook.evaluate(&event).await,
            HookDecision::Block { .. }
        ));
    }

    #[tokio::test]
    async fn test_or_hook_first_allows() {
        // OrHook: allows if EITHER hook allows.
        let hook = ToolBlocklistHook::new(vec!["rm".into()]).or(ToolBlocklistHook::new(vec![])); // second allows everything
        let event = HookEvent::PreToolUse {
            tool_name: "rm".into(), // blocked by first
            input: serde_json::json!({}),
        };
        // Second allows it → Or result is Allow
        assert!(matches!(hook.evaluate(&event).await, HookDecision::Allow));
    }

    #[tokio::test]
    async fn test_boxed_helper() {
        let hook: Box<dyn Hook> = ToolBlocklistHook::new(vec![]).boxed();
        let event = HookEvent::PreToolUse {
            tool_name: "WebSearch".into(),
            input: serde_json::json!({}),
        };
        assert!(matches!(hook.evaluate(&event).await, HookDecision::Allow));
    }
}
