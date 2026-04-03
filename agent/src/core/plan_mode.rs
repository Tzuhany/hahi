// ============================================================================
// Plan Mode — Exploration Before Execution
//
// When a task is complex, the agent can enter plan mode:
//   - Tool set restricted to read-only (search, fetch, query — no write/execute)
//   - Agent explores the problem space and designs an approach
//   - Plan submitted for user review
//   - User approves → exit plan mode → execute with full tools
//   - User modifies → agent revises plan in plan mode
//   - User rejects → run ends
//
// Plan mode is a property of the Run, not the agent.
// The same agent code runs in both modes — only the available tools differ.
//
// Implemented as two built-in tools:
//   EnterPlanMode — LLM calls this when it decides a task needs planning
//   ExitPlanMode  — LLM calls this to submit the plan for review
//
// The agent loop checks run.mode to filter the tool set.
// ============================================================================

use crate::core::r#loop::RunMode;
use crate::llm::ToolDefinition;
use crate::tool::ToolRegistry;

/// Read-only tools allowed in plan mode.
/// These can observe but not modify — safe for exploration.
const PLAN_MODE_TOOLS: &[&str] = &[
    "WebSearch",
    "WebFetch",
    "DbQuery",       // Read-only queries
    "ToolSearch",    // Can discover tools (but not use write tools)
    "EnterPlanMode", // Already in plan mode, but harmless
    "ExitPlanMode",  // To submit the plan
];

/// Get tool definitions appropriate for the current run mode.
///
/// - Execute / Reflection → all tools from registry
/// - Planning             → only read-only tools (safe to explore, can't modify)
pub fn tools_for_mode(registry: &ToolRegistry, mode: RunMode) -> Vec<ToolDefinition> {
    if mode == RunMode::Planning {
        registry
            .api_tool_definitions()
            .into_iter()
            .filter(|t| {
                PLAN_MODE_TOOLS
                    .iter()
                    .any(|a| a.eq_ignore_ascii_case(&t.name))
            })
            .collect()
    } else {
        registry.api_tool_definitions()
    }
}

/// The user's response to a plan review.
///
/// Variants beyond `Approve` are handled by the gateway when it wires
/// `ControlResponse` back to the agent. Not yet connected.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum PlanReviewResponse {
    /// Approve the plan. Exit plan mode, start executing.
    Approve,

    /// Request modifications. Stay in plan mode. Agent revises.
    Modify { feedback: String },

    /// Reject the plan. End the run.
    Reject,
}

/// Format user feedback for injection into the conversation.
///
/// When the user says "modify", their feedback is wrapped in
/// `<plan-feedback>` tags so the LLM knows this is about the plan.
pub fn format_plan_feedback(feedback: &str) -> String {
    crate::core::xml::plan_feedback(feedback)
}

/// System message injected when entering plan mode.
/// Called from the EnterPlanMode tool once it's wired to loop.rs.
#[allow(dead_code)]
pub fn plan_mode_entered_message() -> &'static str {
    "You are now in PLAN MODE. Your tools are restricted to read-only operations. \
     Explore the problem, gather information, and design your approach. \
     When your plan is ready, call ExitPlanMode to submit it for review."
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::ToolRegistry;

    #[test]
    fn test_tools_for_execute_mode_returns_all() {
        let registry = ToolRegistry::new(vec![]);
        let tools = tools_for_mode(&registry, RunMode::Execute);
        // Empty registry → empty tools. Just verifying no panic.
        assert!(tools.is_empty());
    }

    #[test]
    fn test_plan_mode_entered_message_not_empty() {
        assert!(!plan_mode_entered_message().is_empty());
    }

    #[test]
    fn test_format_plan_feedback_wraps() {
        let result = format_plan_feedback("add error handling");
        assert!(result.contains("<plan-feedback>"));
        assert!(result.contains("add error handling"));
    }
}
