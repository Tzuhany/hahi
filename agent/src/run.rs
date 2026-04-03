// ============================================================================
// RunPipeline — Single-Turn Execution Pipeline
//
// Owns the full SendMessage pipeline, step by step:
//
//   1. Load checkpoint        (Redis hot → PG warm)
//   2. Recall memories        (pinned always + conditional RRF)
//   3. Build system prompt    (tools + skills + memory index)
//   4. Assemble tools         (builtin + AgentTool with injected SpawnFn)
//   5. Run agent loop         (core::loop::run_loop)
//   6. Handle control flow    (emit plan / permission requests)
//   7. Save checkpoint        (Redis sync, PG async)
//   8. Background work        (lifecycle decay + reflection, fire-and-forget)
//   9. Expire event stream    (1 hour TTL)
//
// The gRPC handler in service.rs becomes a thin adapter — it just converts
// proto types, calls `RunPipeline::execute`, and wraps the result.
// ============================================================================

use std::sync::{Arc, OnceLock};

use anyhow::Result;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::common::{Checkpoint, ContentBlock, Message, PendingControl, Role};
use crate::core::event_bus::EventBus;
use crate::core::hooks::{HookExt, HookRunner, ToolBlocklistHook};
use crate::core::r#loop::{
    DEFAULT_CONTEXT_WINDOW_TOKENS as CONTEXT_WINDOW_TOKENS, DEFAULT_MODEL, LoopConfigBuilder,
    LoopEvent, LoopRuntime, QueryChain, RunMode, TurnStopReason, run_loop,
};
use crate::infra::metrics::Metrics;
use crate::infra::store::Store;
use crate::llm::LlmProvider;
use crate::memory::types::SessionStats;
use crate::memory::{MemoryEngine, reflect};
use crate::prompt::cache::PromptCache;
use crate::prompt::cache_boundary::join_sections;
use crate::prompt::{PromptConfig, build_system_prompt_with_cache};
use crate::skill::SkillLoader;
use crate::skill::executor::{SkillResult, execute_skill};
use crate::tool::ToolRegistry;
use crate::tool::builtin::{
    AgentTool, MemoryForgetTool, MemorySearchTool, MemoryWriteTool, SpawnFn, WebFetchTool,
    WebSearchTool,
};
use crate::tool::definition::Tool;
use crate::tool::search::ToolSearchTool;

const EVENT_STREAM_TTL_SECS: u64 = 3_600;

// ============================================================================
// Public interface
// ============================================================================

/// Input to a single agent run.
pub struct RunRequest<'a> {
    pub thread_id: &'a str,
    pub user_id: &'a str,
    pub content: &'a str,
    pub message_id: &'a str,
}

/// Output from a successful agent run.
pub struct RunOutput {
    /// Echo of the message_id from the request (for proto response).
    pub message_id: String,
    /// Redis stream key clients subscribe to for SSE events.
    pub stream_key: String,
    /// Final stop reason for this run.
    pub stop_reason: TurnStopReason,
}

/// The pipeline that drives a single SendMessage request end-to-end.
pub struct RunPipeline {
    pub store: Arc<Store>,
    pub provider: Arc<dyn LlmProvider>,
    pub memory: Arc<MemoryEngine>,
    pub skill_loader: Arc<SkillLoader>,
    pub metrics: Arc<Metrics>,
    pub prompt_cache: Arc<Mutex<PromptCache>>,
    pub mcp_tools: Vec<Arc<dyn Tool>>,
}

impl RunPipeline {
    /// Execute one full agent turn.
    pub async fn execute(&self, req: RunRequest<'_>) -> Result<RunOutput> {
        self.execute_with_cancel(req, CancellationToken::new()).await
    }

    /// Execute one full turn using a caller-provided cancellation token.
    pub async fn execute_with_cancel(
        &self,
        req: RunRequest<'_>,
        cancel: CancellationToken,
    ) -> Result<RunOutput> {
        // ── 1. Load or create checkpoint ──────────────────────────────────────
        let mut checkpoint = self.load_or_create_checkpoint(req.thread_id).await?;
        let last_reflection_at = self.store.load_last_reflection_at(req.user_id).await?;

        // ── 2. Prepare memory context + prompt sections ───────────────────────
        let turn_memory = self.memory.prepare_turn(req.user_id, req.content).await;

        // ── 4. Load skills ────────────────────────────────────────────────────
        let skills = self.skill_loader.load_all().await.unwrap_or_default();

        // ── 5. Set up cancel + event bus ──────────────────────────────────────
        let (bus, mut bus_rx) = EventBus::new();

        // Single consumer task: drains the bus and writes to Redis Stream.
        // All loop events — from the main agent and any sub-agents — flow here.
        {
            let store = Arc::clone(&self.store);
            let tid = req.thread_id.to_string();
            tokio::spawn(async move {
                while let Some(event) = bus_rx.recv().await {
                    if let Some((event_type, payload)) = loop_event_to_redis(&event) {
                        let _ = store.emit_event(&tid, event_type, &payload).await;
                    }
                }
            });
        }

        // ── 6. Assemble tool registry ─────────────────────────────────────────
        //
        // Shared memory context — one Arc, three tools.
        let memory_ctx = Arc::clone(&turn_memory.ctx);

        // The SpawnFn breaks the old OnceLock<Weak<LoopConfig>> circular dep.
        // We still use OnceLock because the spawn closure captures `loop_config`,
        // which can only be built after the tool registry is ready. The cell is
        // filled immediately after LoopConfig is constructed (a few lines below).
        let spawn_cell: Arc<OnceLock<SpawnFn>> = Arc::new(OnceLock::new());

        let tool_registry =
            self.build_tool_registry(Arc::clone(&spawn_cell), Arc::clone(&memory_ctx));

        // ── 7. Build system prompt ─────────────────────────────────────────────
        let prompt_sections = {
            let mut cache = self.prompt_cache.lock().await;
            build_system_prompt_with_cache(
                &PromptConfig {
                    tool_registry: &tool_registry,
                    skills: &skills,
                    memory_index: turn_memory.index_section.as_deref(),
                    user_instructions: "",
                    project_instructions: "",
                    rules: &[],
                    context_window_tokens: CONTEXT_WINDOW_TOKENS,
                    model_name: DEFAULT_MODEL,
                },
                &mut cache,
            )
        };
        let mut system_prompt = join_sections(&prompt_sections);
        if let Some(recalled) = turn_memory.recalled_section.as_deref() {
            system_prompt.push_str("\n\n");
            system_prompt.push_str(recalled);
        }
        system_prompt.push_str("\n\n");
        system_prompt.push_str(turn_memory.write_guidance);

        // ── 8. Build LoopConfig ───────────────────────────────────────────────
        let loop_config =
            LoopConfigBuilder::new(Arc::clone(&self.provider), tool_registry, system_prompt)
                .model(DEFAULT_MODEL)
                .metrics(Arc::clone(&self.metrics))
                .build();

        // ── 9. Build LoopRuntime + wire SpawnFn ───────────────────────────────
        //
        // LoopRuntime is Clone (cheap — wraps CancellationToken + UnboundedSender).
        // The spawn closure captures a clone; sub-agents get child_bus automatically.
        let runtime = LoopRuntime::new(cancel, bus);

        let spawn_fn: SpawnFn = {
            let config = Arc::clone(&loop_config);
            let rt = runtime.clone();
            Arc::new(
                move |agent_type: String, prompt: String, parent_messages: Vec<Message>| {
                    let config = Arc::clone(&config);
                    let rt = rt.clone();
                    Box::pin(async move {
                        crate::multi::spawn::spawn_sub_agent(
                            &agent_type,
                            &prompt,
                            parent_messages,
                            &config,
                            &rt,
                        )
                        .await
                    })
                },
            )
        };
        let _ = spawn_cell.set(spawn_fn);

        // ── 10. Hooks ─────────────────────────────────────────────────────────
        let hooks = HookRunner::new(vec![ToolBlocklistHook::new(vec![]).boxed()]);

        // ── 11. Handle slash-command / skill invocation ────────────────────────
        if req.content.starts_with('/') {
            let (skill_name, args) = parse_slash_command(req.content);
            if let Ok(Some((skill, _))) = self.skill_loader.get_with_prompt(&skill_name).await {
                let args_opt = if args.is_empty() {
                    None
                } else {
                    Some(args.as_str())
                };
                match execute_skill(&skill, args_opt, &checkpoint.recent_messages).await {
                    Ok(SkillResult::Inline { messages: extra }) => {
                        checkpoint.recent_messages.extend(extra);
                    }
                    Ok(SkillResult::Forked { output }) => {
                        tracing::debug!(skill = skill.name, output, "forked skill output");
                    }
                    Err(e) => {
                        tracing::warn!(skill = skill_name, error = %e, "skill execution failed");
                    }
                }
            }
        }

        // ── 12. Push user message + run the loop ───────────────────────────────
        checkpoint
            .recent_messages
            .push(Message::user(req.message_id, req.content));

        self.metrics.run_started();
        let result = run_loop(
            &loop_config,
            &runtime,
            &mut checkpoint.recent_messages,
            &hooks,
        )
        .await;
        self.metrics.run_ended();
        let result = result?;
        let pending_control = runtime.take_pending_control();

        tracing::info!(
            thread_id = req.thread_id,
            input_tokens = result.usage.input_tokens,
            output_tokens = result.usage.output_tokens,
            total_tokens = result.usage.total(),
            iterations = result.iterations,
            stop_reason = ?result.stop_reason,
            "run completed"
        );

        // ── 13. Save checkpoint ────────────────────────────────────────────────
        checkpoint.pending_control = match result.stop_reason {
            TurnStopReason::RequiresAction { .. } | TurnStopReason::PlanReview { .. } => {
                pending_control
            }
            _ => None,
        };
        checkpoint.total_input_tokens += result.usage.input_tokens;
        checkpoint.total_output_tokens += result.usage.output_tokens;
        self.save_checkpoint(&checkpoint).await?;

        // ── 14. Background: lifecycle + reflection (fire-and-forget) ──────────
        self.memory.spawn_lifecycle(req.user_id.to_string());

        let stats = SessionStats {
            turn_count: result.iterations,
            memories_written_this_run: runtime.stats.memories_written_this_run(),
            last_reflection_at,
        };
        if self.memory.should_reflect(&stats) {
            let store = Arc::clone(&self.store);
            let provider = Arc::clone(&self.provider);
            let memory = Arc::clone(&self.memory);
            let uid = req.user_id.to_string();
            let memory_idx_str = turn_memory.index_section.clone().unwrap_or_default();
            let convo_text = format_conversation_for_reflection(&checkpoint.recent_messages);
            tokio::spawn(async move {
                run_reflection(store, provider, memory, &uid, &memory_idx_str, &convo_text).await;
            });
        }

        // ── 15. Expire event stream ────────────────────────────────────────────
        let _ = self
            .store
            .expire_event_stream(req.thread_id, EVENT_STREAM_TTL_SECS)
            .await;

        Ok(RunOutput {
            message_id: req.message_id.to_string(),
            stream_key: format!("results:{}", req.thread_id),
            stop_reason: result.stop_reason,
        })
    }
}

// ============================================================================
// Checkpoint helpers
// ============================================================================

impl RunPipeline {
    fn build_tool_registry(
        &self,
        spawn_cell: Arc<OnceLock<SpawnFn>>,
        memory_ctx: Arc<crate::memory::MemoryCtx>,
    ) -> Arc<ToolRegistry> {
        let mut searchable_tools: Vec<Arc<dyn Tool>> = vec![
            Arc::new(AgentTool::new(Arc::clone(&spawn_cell))),
            Arc::new(WebFetchTool::new()),
            Arc::new(WebSearchTool::new()),
            Arc::new(MemoryWriteTool::new(Arc::clone(&memory_ctx))),
            Arc::new(MemorySearchTool::new(Arc::clone(&memory_ctx))),
            Arc::new(MemoryForgetTool::new(Arc::clone(&memory_ctx))),
        ];
        searchable_tools.extend(self.mcp_tools.iter().cloned());

        let search_registry = Arc::new(ToolRegistry::new(searchable_tools.clone()));
        let mut tools = searchable_tools;
        tools.push(Arc::new(ToolSearchTool::new(search_registry)));

        Arc::new(ToolRegistry::new(tools))
    }

    async fn load_checkpoint(&self, thread_id: &str) -> Result<Option<Checkpoint>> {
        if let Some(bytes) = self.store.redis_load_checkpoint(thread_id).await? {
            return Ok(Some(serde_json::from_slice(&bytes)?));
        }
        if let Some(bytes) = self.store.pg_load_checkpoint(thread_id).await? {
            return Ok(Some(serde_json::from_slice(&bytes)?));
        }
        Ok(None)
    }

    async fn load_or_create_checkpoint(&self, thread_id: &str) -> Result<Checkpoint> {
        Ok(self
            .load_checkpoint(thread_id)
            .await?
            .unwrap_or_else(|| Checkpoint {
                thread_id: thread_id.to_string(),
                compact_summary: None,
                recent_messages: Vec::new(),
                total_input_tokens: 0,
                total_output_tokens: 0,
                compact_count: 0,
                forked_from: None,
                pending_control: None,
            }))
    }

    async fn save_checkpoint(&self, checkpoint: &Checkpoint) -> Result<()> {
        let bytes = serde_json::to_vec(checkpoint)?;
        self.store
            .redis_save_checkpoint(&checkpoint.thread_id, &bytes)
            .await?;
        let store = Arc::clone(&self.store);
        let tid = checkpoint.thread_id.clone();
        let bytes_clone = bytes.clone();
        tokio::spawn(async move {
            if let Err(e) = store.pg_save_checkpoint(&tid, &bytes_clone).await {
                tracing::warn!(error = %e, "async PG checkpoint write failed");
            }
        });
        Ok(())
    }
}

// ============================================================================
// Private helpers
// ============================================================================

/// Parse "/skill-name args" → ("skill-name", "args").
pub(crate) fn parse_slash_command(content: &str) -> (String, String) {
    let without_slash = content.trim_start_matches('/');
    match without_slash.split_once(' ') {
        Some((name, args)) => (name.to_string(), args.to_string()),
        None => (without_slash.to_string(), String::new()),
    }
}

/// Convert a LoopEvent to (event_type, JSON payload) for Redis Streams.
pub(crate) fn loop_event_to_redis(event: &LoopEvent) -> Option<(&'static str, serde_json::Value)> {
    match event {
        LoopEvent::SessionStateChanged { state } => Some((
            "session_state_changed",
            serde_json::json!({ "state": state }),
        )),
        LoopEvent::ControlRequest {
            request_id,
            kind,
            payload,
        } => Some((
            "control_request",
            serde_json::json!({
                "request_id": request_id,
                "type": kind,
                "payload": payload,
            }),
        )),
        LoopEvent::Stream(stream_event) => {
            use crate::common::StreamEvent;
            let payload = match stream_event {
                StreamEvent::TextDelta { text } => {
                    serde_json::json!({ "type": "text_delta", "text": text })
                }
                StreamEvent::ThinkingDelta { text } => {
                    serde_json::json!({ "type": "thinking_delta", "text": text })
                }
                StreamEvent::ToolUseStart { id, name } => {
                    serde_json::json!({ "type": "tool_use_start", "id": id, "name": name })
                }
                StreamEvent::ToolInputDelta { id, json_chunk } => {
                    serde_json::json!({ "type": "tool_input_delta", "id": id, "json_chunk": json_chunk })
                }
                StreamEvent::ToolUseEnd { id } => {
                    serde_json::json!({ "type": "tool_use_end", "id": id })
                }
                StreamEvent::MessageEnd { usage, stop_reason } => serde_json::json!({
                    "type": "message_end",
                    "stop_reason": stop_reason.to_string(),
                    "input_tokens": usage.input_tokens,
                    "output_tokens": usage.output_tokens,
                }),
                StreamEvent::Error {
                    message,
                    is_retryable,
                } => {
                    serde_json::json!({ "type": "error", "message": message, "is_retryable": is_retryable })
                }
            };
            Some(("stream", payload))
        }
        LoopEvent::ToolStart {
            id,
            name,
            input_preview,
        } => Some((
            "tool_start",
            serde_json::json!({ "id": id, "name": name, "input_preview": input_preview }),
        )),
        LoopEvent::ToolResult {
            id,
            name,
            content,
            is_error,
        } => Some((
            "tool_result",
            serde_json::json!({ "id": id, "name": name, "content": content, "is_error": is_error }),
        )),
        LoopEvent::Compacted { pre_tokens } => {
            Some(("compacted", serde_json::json!({ "pre_tokens": pre_tokens })))
        }
        LoopEvent::Collapsed { folded_count } => Some((
            "collapsed",
            serde_json::json!({ "folded_count": folded_count }),
        )),
        LoopEvent::HookBlocked { hook_name, reason } => Some((
            "hook_blocked",
            serde_json::json!({ "hook_name": hook_name, "reason": reason }),
        )),
        LoopEvent::PlanModeChanged { mode } => {
            Some(("plan_mode_changed", serde_json::json!({ "mode": mode })))
        }
        LoopEvent::TurnEnd { stop_reason, usage } => Some((
            "turn_end",
            serde_json::json!({
                "stop_reason": format!("{:?}", stop_reason),
                "total_tokens": usage.total(),
            }),
        )),
    }
}

/// Flatten conversation into plain text for the reflection prompt.
fn format_conversation_for_reflection(messages: &[Message]) -> String {
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
async fn run_reflection(
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
    let memory_tools: Vec<Arc<dyn crate::tool::definition::Tool>> = vec![
        Arc::new(MemoryWriteTool::new(Arc::clone(&memory_ctx))),
        Arc::new(MemoryForgetTool::new(Arc::clone(&memory_ctx))),
    ];
    let tool_registry = Arc::new(ToolRegistry::new(memory_tools));

    let loop_config = LoopConfigBuilder::new(Arc::clone(&provider), tool_registry, system_prompt)
        .model(DEFAULT_MODEL)
        .max_tokens(4_096)
        .max_iterations(1)
        .run_mode(RunMode::Reflection)
        .chain(QueryChain {
            chain_id: uuid::Uuid::new_v4().to_string(),
            depth: 0,
            max_depth: 1,
        })
        .build();

    // Reflection doesn't stream to any consumer; drop the receiver immediately.
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
