// AgentTool — spawns a specialized sub-agent for a subtask.
//
// Dependency injection via SpawnFn breaks the circular reference that used to
// require OnceLock<Weak<LoopConfig>>:
//
//   Before: LoopConfig → ToolRegistry → AgentTool → Weak<LoopConfig>
//   After:  LoopConfig → ToolRegistry → AgentTool → SpawnFn (opaque closure)
//
// The SpawnFn is built *after* LoopConfig exists and captures it (+ cancel +
// emit) via Arc. AgentTool itself never imports LoopConfig — it only knows
// "call this fn to spawn a sub-agent".

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use futures::future::BoxFuture;
use serde_json::Value;

use crate::common::{Message, ToolContext, ToolOutput, ToolProgress};
use crate::systems::tools::definition::Tool;

/// The spawn capability injected into AgentTool.
///
/// `(agent_type, prompt, parent_messages) → Future<Result<output_text>>`
///
/// The closure captures provider, config, cancel, and emit — all the
/// context needed to spawn a sub-agent — without exposing those types
/// to AgentTool directly.
pub type SpawnFn = Arc<
    dyn Fn(String, String, Vec<Message>) -> BoxFuture<'static, anyhow::Result<String>>
        + Send
        + Sync,
>;

pub struct AgentTool {
    /// Injected exactly once after LoopConfig is constructed.
    spawn: Arc<OnceLock<SpawnFn>>,
}

impl AgentTool {
    pub fn new(spawn: Arc<OnceLock<SpawnFn>>) -> Self {
        Self { spawn }
    }
}

#[async_trait]
impl Tool for AgentTool {
    fn name(&self) -> &str {
        "Agent"
    }

    fn description(&self) -> &str {
        "Spawn a specialized sub-agent to handle a focused subtask in parallel."
    }

    fn prompt(&self) -> String {
        "Use this tool to delegate complex subtasks to specialized sub-agents.\n\
         Available agent types:\n\
         - general: full capability, inherits parent model\n\
         - explorer: read-only tools, for research and investigation\n\
         - planner: read-only tools, for planning and design\n\n\
         The sub-agent runs in its own context with isolated message history.\n\
         Multiple sub-agents can run concurrently."
            .to_string()
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "agent_type": {
                    "type": "string",
                    "description": "Type of sub-agent: 'general', 'explorer', or 'planner'",
                    "enum": ["general", "explorer", "planner"]
                },
                "prompt": {
                    "type": "string",
                    "description": "Complete task description for the sub-agent"
                }
            },
            "required": ["prompt"]
        })
    }

    fn should_defer(&self) -> bool {
        false
    }

    fn search_hint(&self) -> Option<&str> {
        Some("delegate subtask to specialized sub-agent")
    }

    fn is_concurrent(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let spawn = match self.spawn.get() {
            Some(f) => Arc::clone(f),
            None => return ToolOutput::error("internal error: AgentTool spawn not initialized"),
        };

        let agent_type = input["agent_type"]
            .as_str()
            .unwrap_or("general")
            .to_string();
        let prompt = match input["prompt"].as_str() {
            Some(p) if !p.is_empty() => p.to_string(),
            _ => return ToolOutput::error("prompt is required and must be non-empty"),
        };

        (ctx.on_progress)(ToolProgress {
            message: format!("spawning {agent_type} sub-agent"),
        });

        if ctx.cancel.is_cancelled() {
            return ToolOutput::error("cancelled before sub-agent could start");
        }

        match spawn(agent_type, prompt, ctx.message_history.clone()).await {
            Ok(result) => ToolOutput::success(result)
                .with_artifacts(vec![])
                .with_messages(vec![]),
            Err(e) => ToolOutput::error(format!("sub-agent failed: {e}")),
        }
    }
}
