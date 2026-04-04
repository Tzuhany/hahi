use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::adapters::llm::LlmProvider;
use crate::adapters::store::Store;
use crate::common::{ContentBlock, Message, Role};
use crate::kernel::event_bus::EventBus;
use crate::kernel::hooks::HookRunner;
use crate::kernel::r#loop::{
    LoopConfigBuilder, LoopRuntime, QueryChain, RunMode, default_model, run_loop,
};
use crate::systems::memory::{MemoryEngine, reflect};
use crate::systems::tools::builtin::{MemoryForgetTool, MemoryWriteTool};
use crate::systems::tools::registry::ToolRegistry;

/// Flatten conversation into plain text for the reflection prompt.
pub fn format_conversation_for_reflection(messages: &[Message]) -> String {
    messages
        .iter()
        .filter_map(|m| {
            let role = match m.role {
                Role::User => "User",
                Role::Assistant => "Assistant",
                Role::System => return None,
            };
            let text = m
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            if text.is_empty() {
                None
            } else {
                Some(format!("{role}: {text}"))
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Run a background memory reflection mini-turn.
pub async fn run_reflection_turn(
    store: Arc<Store>,
    provider: Arc<dyn LlmProvider>,
    memory: Arc<MemoryEngine>,
    agent_id: &str,
    memory_idx_str: &str,
    convo_text: &str,
) {
    let system_prompt = reflect::reflection_system_prompt(memory_idx_str);
    let user_msg = reflect::reflection_user_message(convo_text);

    let memory_ctx = memory.tool_ctx(agent_id);
    let memory_tools: Vec<Arc<dyn crate::systems::tools::definition::Tool>> = vec![
        Arc::new(MemoryWriteTool::new(Arc::clone(&memory_ctx))),
        Arc::new(MemoryForgetTool::new(Arc::clone(&memory_ctx))),
    ];
    let tool_registry = Arc::new(ToolRegistry::new(memory_tools));

    let loop_config = LoopConfigBuilder::new(Arc::clone(&provider), tool_registry, system_prompt)
        .model(default_model())
        .max_tokens(4_096)
        .max_iterations(1)
        .run_mode(RunMode::Reflection)
        .chain(QueryChain {
            chain_id: uuid::Uuid::new_v4().to_string(),
            depth: 0,
            max_depth: 1,
        })
        .build();

    let (refl_bus, _refl_rx) = EventBus::new();
    let refl_runtime = LoopRuntime::new(CancellationToken::new(), refl_bus);
    let hooks = HookRunner::empty();
    let mut messages = vec![Message::user(uuid::Uuid::new_v4().to_string(), user_msg)];

    match run_loop(&loop_config, &refl_runtime, &mut messages, &hooks).await {
        Ok(r) => {
            if let Err(e) = store
                .save_last_reflection_at(agent_id, reflect::now())
                .await
            {
                tracing::warn!(agent_id, error = %e, "failed to persist reflection timestamp");
            }
            tracing::debug!(agent_id, iterations = r.iterations, "reflection completed");
        }
        Err(e) => tracing::warn!(agent_id, error = %e, "reflection failed"),
    }
}
