// ============================================================================
// Sub-Agent Spawning
//
// Sub-agents are tokio::spawn'd async tasks, NOT separate processes.
// They reuse the same kernel loop with different config.
//
// This is Claude Code's key insight: a sub-agent is just
// "the same loop with a different prompt, tool set, and model."
// No IPC, no serialization, no message passing overhead.
//
// The parent agent's LLM returns an AgentTool call → we spawn a sub-agent:
//   1. Look up the agent definition (agents/*.yaml)
//   2. Build isolated config (isolation.rs)
//   3. tokio::spawn(run_loop(...))
//   4. Await result → return as tool_result to parent
//
// Multiple sub-agents can run concurrently (e.g., "explore A and B in parallel").
// Each gets its own message history and token tracking.
// All share the same LLM client and tool registry (via Arc).
// ============================================================================

use anyhow::Result;

use std::sync::Arc;
use std::time::Instant;

use crate::common::Message;
use crate::kernel::r#loop::{LoopConfig, LoopEvent, LoopRuntime, TurnResult, run_loop};
use crate::systems::subagents::agent_def::find_agent_def;
use crate::systems::subagents::fork::build_fork_messages;
use crate::systems::subagents::isolation::build_sub_agent_config;

/// Spawn a sub-agent and wait for its result.
///
/// Called from the tool executor when the LLM invokes the Agent tool.
///
/// # Arguments
/// * `agent_type` — name of the agent definition (e.g., "explorer")
/// * `prompt` — the task description from the LLM
/// * `parent_config` — the parent agent's loop config (for sharing providers)
/// * `parent_runtime` — the parent's runtime; child inherits cancel + gets a
///   filtered child bus that forwards tool events with prefixed IDs.
///
/// # Returns
/// The sub-agent's final text output as a string (for use as tool_result).
pub async fn spawn_sub_agent(
    agent_type: &str,
    prompt: &str,
    parent_messages: Vec<Message>,
    parent_config: &LoopConfig,
    parent_runtime: &LoopRuntime,
) -> Result<String> {
    // Look up agent definition.
    let def = find_agent_def(agent_type).unwrap_or_else(|| {
        tracing::warn!(agent_type, "unknown agent type, falling back to general");
        find_agent_def("general").expect("general agent definition must exist")
    });

    let task_id = uuid::Uuid::new_v4().to_string();

    // Notify parent that a sub-agent is starting.
    parent_runtime.bus.emit(LoopEvent::ToolStart {
        id: task_id.clone(),
        name: format!("Agent({})", def.name),
        input_preview: truncate_preview(prompt, 100),
    });

    // Build isolated config. Returns None if max depth exceeded.
    let Some(sub_config) = build_sub_agent_config(parent_config, &def) else {
        let msg = format!(
            "Cannot spawn sub-agent '{}': max nesting depth ({}) exceeded",
            def.name, parent_config.chain.max_depth
        );
        parent_runtime.bus.emit(LoopEvent::ToolResult {
            id: task_id,
            name: format!("Agent({})", def.name),
            content: msg.clone(),
            is_error: true,
        });
        return Ok(msg);
    };

    // Child cancel inherits from parent so cancelling the parent stops all children.
    let child_cancel = parent_runtime.cancel.child_token();

    // child_bus forwards filtered events to parent with task_id-prefixed IDs.
    // Text/thinking deltas stay local — client sees tool activity only.
    let child_bus = parent_runtime.bus.child_bus(task_id.clone());

    let sub_runtime =
        LoopRuntime::with_stats(child_cancel, child_bus, Arc::clone(&parent_runtime.stats));
    let start_time = Instant::now();

    // Reuse the parent's prefix when available so sibling sub-agents can share
    // the same cached prompt prefix.
    let mut messages = if parent_messages.is_empty() {
        vec![Message::user(
            uuid::Uuid::new_v4().to_string(),
            prompt.to_string(),
        )]
    } else {
        build_fork_messages(&parent_messages, prompt)
    };

    // Run the sub-agent loop in a spawned task.
    let result = tokio::spawn(async move {
        let hooks = crate::kernel::hooks::HookRunner::empty();
        run_loop(&sub_config, &sub_runtime, &mut messages, &hooks).await
    })
    .await??;

    // Extract the final text from the sub-agent's last assistant message.
    let output = extract_final_text(&result);

    // Notify parent that the sub-agent is done.
    let status = match &result.stop_reason {
        crate::kernel::r#loop::TurnStopReason::Completed => "completed",
        crate::kernel::r#loop::TurnStopReason::Cancelled => "stopped",
        _ => "failed",
    };

    let duration_ms = start_time.elapsed().as_millis() as u64;
    let notification = crate::kernel::xml::task_notification_with_usage(
        &task_id,
        status,
        &output,
        result.usage.total(),
        result.iterations,
        duration_ms,
    );
    parent_runtime.bus.emit(LoopEvent::ToolResult {
        id: task_id,
        name: format!("Agent({})", def.name),
        content: notification,
        is_error: status == "failed",
    });

    Ok(output)
}

/// Extract the final text output from a turn result.
///
/// Looks at the last assistant message and concatenates its text blocks.
fn extract_final_text(result: &TurnResult) -> String {
    result
        .messages
        .iter()
        .rev()
        .find(|m| m.role == crate::common::Role::Assistant)
        .map(|m| {
            m.content
                .iter()
                .filter_map(|b| match b {
                    crate::common::ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<&str>>()
                .join("")
        })
        .unwrap_or_default()
}

/// Truncate a string for preview display.
fn truncate_preview(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_preview_short() {
        assert_eq!(truncate_preview("hello", 100), "hello");
    }

    #[test]
    fn test_truncate_preview_long() {
        let long = "x".repeat(200);
        let preview = truncate_preview(&long, 50);
        assert!(preview.len() <= 50);
        assert!(preview.ends_with("..."));
    }
}
