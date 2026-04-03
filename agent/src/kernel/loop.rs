// ============================================================================
// Agent Main Loop
//
// The beating heart of the agent. Everything else exists to serve this loop.
//
// Philosophy:
//   "The framework is an executor, not a thinker."
//   The LLM decides what to do. This loop executes and feeds results back.
//
// Structure:
//   run_loop()            — public entry point; orchestrates phases
//   ├── StreamProcessor   — state machine that consumes one LLM stream
//   │   ├── on_tool_end() → dispatch_tool() → permission + hook + submit
//   │   └── polls executor between events (true streaming execution)
//   ├── finalize_turn()   — PreComplete hook + plan review + completion
//   └── collect_tool_results() — await remaining tools, PostToolUse hooks
//
// LoopConfig  — static, Arc-shareable (model, tools, permissions, metrics)
// LoopRuntime — per-turn context (cancel signal, event bus)
// ============================================================================

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::Result;
use futures::{Stream, StreamExt};
use tokio_util::sync::CancellationToken;

use crate::adapters::llm::{LlmProvider, ProviderConfig};
use crate::adapters::metrics::Metrics;
use crate::common::{ContentBlock, Message, PendingControl, StopReason, StreamEvent, TokenUsage};
use crate::kernel::compression::{CompressionPipeline, StageEvent};
use crate::kernel::context::{ContextManager, ContextPressure};
use crate::kernel::error_recovery::{
    self, ApiErrorKind, LoopControl, RecoveryAction, RecoveryState, continuation_prompt,
    effective_max_tokens,
};
use crate::kernel::event_bus::EventBus;
use crate::kernel::hooks::{HookDecision, HookEvent, HookRunner};
use crate::kernel::permission::{PermissionDecision, PermissionEvaluator};
use crate::kernel::plan_mode;
use crate::systems::tools::executor::{CompletedTool, PendingTool, ToolExecutor};
use crate::systems::tools::registry::ToolRegistry;

// ============================================================================
// Public types
// ============================================================================

/// Run mode: governs tool availability and completion semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// Full tool access. Normal agent execution.
    Execute,
    /// Read-only tools only. LLM plans before acting.
    Planning,
    /// Memory-write tools only. Background reflection mini-run.
    Reflection,
}

/// Configuration for a single agent loop run.
pub struct LoopConfig {
    pub provider: Arc<dyn LlmProvider>,
    /// Compression pipeline (L1 budget + L2 collapse + L3 compact).
    /// Built once and reused across iterations.
    pub compression: CompressionPipeline,
    pub tool_registry: Arc<ToolRegistry>,
    pub system_prompt: String,
    pub model: String,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
    pub provider_extensions: HashMap<String, serde_json::Value>,
    pub context_window_tokens: usize,
    pub max_iterations: u32,
    pub cwd: std::path::PathBuf,
    /// Fraction of context window at which compaction triggers (default 0.7).
    pub compact_threshold: f64,
    pub run_mode: RunMode,
    /// Query chain tracking — position in the sub-agent tree.
    pub chain: QueryChain,
    /// Permission evaluator. Sub-agents inherit from parent.
    pub permission: Arc<PermissionEvaluator>,
    /// Shared metrics. All sub-agents accumulate into the same counters.
    pub metrics: Arc<Metrics>,
}

/// Position in the sub-agent tree.
#[derive(Debug, Clone)]
pub struct QueryChain {
    pub chain_id: String,
    /// 0 = top-level, increments with each spawn.
    pub depth: u32,
    pub max_depth: u32,
}

impl QueryChain {
    /// Create a fresh root chain (depth 0, new chain ID).
    pub fn root() -> Self {
        Self {
            chain_id: new_id(),
            depth: 0,
            max_depth: 5,
        }
    }

    /// Create a child chain (increments depth, shares chain ID).
    pub fn child(&self) -> Self {
        Self {
            chain_id: self.chain_id.clone(),
            depth: self.depth + 1,
            max_depth: self.max_depth,
        }
    }
}

/// Result of a completed agent turn.
pub struct TurnResult {
    pub messages: Vec<Message>,
    pub stop_reason: TurnStopReason,
    pub usage: TokenUsage,
    pub iterations: u32,
}

impl TurnResult {
    /// Convenience constructor — avoids repeating `messages.clone()` everywhere.
    fn build(
        messages: &[Message],
        stop_reason: TurnStopReason,
        usage: TokenUsage,
        iterations: u32,
    ) -> Self {
        Self {
            messages: messages.to_vec(),
            stop_reason,
            usage,
            iterations,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnStopReason {
    Completed,
    MaxIterations,
    Cancelled,
    ContextOverflow,
    DiminishingReturns,
    RequiresAction { request_id: String },
    PlanReview { plan: String },
    Error(String),
}

// ============================================================================
// LoopConfigBuilder — ergonomic LoopConfig construction
//
// Sensible defaults for every optional field. Required fields (provider,
// tool_registry, system_prompt) are the only constructor arguments.
// CompressionPipeline is built automatically from the provider.
//
//   let config = LoopConfigBuilder::new(provider, registry, prompt)
//       .model("claude-opus-4-6")
//       .max_tokens(16_384)
//       .metrics(Arc::clone(&metrics))
//       .build();
// ============================================================================

/// Default values shared between LoopConfigBuilder and run.rs.
pub const DEFAULT_MODEL: &str = "claude-opus-4-6";
pub const DEFAULT_CONTEXT_WINDOW_TOKENS: usize = 200_000;
pub const DEFAULT_MAX_ITERATIONS: u32 = 50;
pub const DEFAULT_COMPACT_THRESHOLD: f64 = 0.7;
pub const DEFAULT_MAX_TOKENS: u32 = 16_384;

/// Builder for [`LoopConfig`].
#[allow(dead_code)]
pub struct LoopConfigBuilder {
    provider: Arc<dyn LlmProvider>,
    tool_registry: Arc<ToolRegistry>,
    system_prompt: String,
    model: String,
    max_tokens: u32,
    temperature: Option<f32>,
    provider_extensions: HashMap<String, serde_json::Value>,
    context_window_tokens: usize,
    max_iterations: u32,
    cwd: std::path::PathBuf,
    compact_threshold: f64,
    run_mode: RunMode,
    chain: QueryChain,
    permission: Arc<PermissionEvaluator>,
    metrics: Arc<Metrics>,
}

impl LoopConfigBuilder {
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        tool_registry: Arc<ToolRegistry>,
        system_prompt: impl Into<String>,
    ) -> Self {
        Self {
            provider,
            tool_registry,
            system_prompt: system_prompt.into(),
            model: DEFAULT_MODEL.to_string(),
            max_tokens: DEFAULT_MAX_TOKENS,
            temperature: None,
            provider_extensions: HashMap::new(),
            context_window_tokens: DEFAULT_CONTEXT_WINDOW_TOKENS,
            max_iterations: DEFAULT_MAX_ITERATIONS,
            cwd: std::path::PathBuf::from("/tmp"),
            compact_threshold: DEFAULT_COMPACT_THRESHOLD,
            run_mode: RunMode::Execute,
            chain: QueryChain::root(),
            permission: Arc::new(PermissionEvaluator::auto()),
            metrics: Arc::new(Metrics::new()),
        }
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn max_tokens(mut self, v: u32) -> Self {
        self.max_tokens = v;
        self
    }

    pub fn max_iterations(mut self, v: u32) -> Self {
        self.max_iterations = v;
        self
    }

    pub fn run_mode(mut self, mode: RunMode) -> Self {
        self.run_mode = mode;
        self
    }

    pub fn chain(mut self, chain: QueryChain) -> Self {
        self.chain = chain;
        self
    }

    pub fn metrics(mut self, m: Arc<Metrics>) -> Self {
        self.metrics = m;
        self
    }

    // ── Optional overrides ────────────────────────────────────────────────────
    // Not called by current production code, but part of the public builder API.
    // External callers and future integration tests use these.

    #[allow(dead_code)]
    pub fn temperature(mut self, v: f32) -> Self {
        self.temperature = Some(v);
        self
    }

    #[allow(dead_code)]
    pub fn context_window_tokens(mut self, v: usize) -> Self {
        self.context_window_tokens = v;
        self
    }

    #[allow(dead_code)]
    pub fn cwd(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.cwd = path.into();
        self
    }

    #[allow(dead_code)]
    pub fn compact_threshold(mut self, v: f64) -> Self {
        self.compact_threshold = v;
        self
    }

    #[allow(dead_code)]
    pub fn permission(mut self, p: Arc<PermissionEvaluator>) -> Self {
        self.permission = p;
        self
    }

    #[allow(dead_code)]
    pub fn provider_extensions(mut self, ext: HashMap<String, serde_json::Value>) -> Self {
        self.provider_extensions = ext;
        self
    }

    /// Build the [`LoopConfig`], automatically constructing the compression pipeline.
    pub fn build(self) -> Arc<LoopConfig> {
        let compression = crate::kernel::compression::CompressionPipeline::standard(
            Arc::clone(&self.provider),
            None,
        );
        Arc::new(LoopConfig {
            compression,
            provider: self.provider,
            tool_registry: self.tool_registry,
            system_prompt: self.system_prompt,
            model: self.model,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            provider_extensions: self.provider_extensions,
            context_window_tokens: self.context_window_tokens,
            max_iterations: self.max_iterations,
            cwd: self.cwd,
            compact_threshold: self.compact_threshold,
            run_mode: self.run_mode,
            chain: self.chain,
            permission: self.permission,
            metrics: self.metrics,
        })
    }
}

/// Per-turn runtime context. Separate from LoopConfig (which is static + Arc-shared).
#[derive(Default)]
pub struct LoopStats {
    memories_written_this_run: AtomicU32,
}

impl LoopStats {
    pub fn record_memory_write(&self) {
        self.memories_written_this_run
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn memories_written_this_run(&self) -> u32 {
        self.memories_written_this_run.load(Ordering::Relaxed)
    }
}

/// Per-turn runtime context. Separate from LoopConfig (which is static + Arc-shared).
///
/// Every call to run_loop gets its own LoopRuntime.
/// Sub-agents receive a child runtime via EventBus::child_bus + a child cancel token.
#[derive(Clone)]
pub struct LoopRuntime {
    /// Cancellation signal. Agent stops cleanly when cancelled.
    pub cancel: CancellationToken,
    /// Event bus. All events emitted by this loop go here.
    pub bus: EventBus,
    /// Per-run counters shared with sub-agents.
    pub stats: Arc<LoopStats>,
    /// Structured control request captured during this run, if any.
    pub pending_control: Arc<Mutex<Option<PendingControl>>>,
}

impl LoopRuntime {
    pub fn new(cancel: CancellationToken, bus: EventBus) -> Self {
        Self::with_stats(cancel, bus, Arc::new(LoopStats::default()))
    }

    pub fn with_stats(cancel: CancellationToken, bus: EventBus, stats: Arc<LoopStats>) -> Self {
        Self {
            cancel,
            bus,
            stats,
            pending_control: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_pending_control(&self, control: PendingControl) {
        if let Ok(mut slot) = self.pending_control.lock() {
            *slot = Some(control);
        }
    }

    pub fn take_pending_control(&self) -> Option<PendingControl> {
        self.pending_control
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
    }
}

/// Events emitted by the loop for external consumption.
#[derive(Debug, Clone)]
pub enum LoopEvent {
    SessionStateChanged {
        state: String,
    },
    ControlRequest {
        request_id: String,
        kind: String,
        payload: serde_json::Value,
    },
    Stream(StreamEvent),
    ToolStart {
        id: String,
        name: String,
        input_preview: String,
    },
    ToolResult {
        id: String,
        name: String,
        content: String,
        is_error: bool,
    },
    Compacted {
        pre_tokens: usize,
    },
    Collapsed {
        folded_count: usize,
    },
    HookBlocked {
        hook_name: String,
        reason: String,
    },
    PlanModeChanged {
        mode: String,
    },
    TurnEnd {
        stop_reason: TurnStopReason,
        usage: TokenUsage,
    },
}

// ============================================================================
// Main loop — public entry point
// ============================================================================

/// Run the agent loop for one turn.
///
/// Calls the LLM in a `loop { ... }` until the model returns EndTurn without
/// tool_use (or hits a stopping condition). Tools execute concurrently with
/// the LLM's streaming output.
pub async fn run_loop(
    config: &LoopConfig,
    runtime: &LoopRuntime,
    messages: &mut Vec<Message>,
    hooks: &HookRunner,
) -> Result<TurnResult> {
    runtime.bus.emit(LoopEvent::SessionStateChanged {
        state: if config.run_mode == RunMode::Planning {
            "planning".into()
        } else {
            "running".into()
        },
    });
    if config.run_mode == RunMode::Planning {
        runtime.bus.emit(LoopEvent::PlanModeChanged {
            mode: "planning".into(),
        });
    }

    let ctx =
        ContextManager::new(config.context_window_tokens).with_threshold(config.compact_threshold);
    let tool_defs = plan_mode::tools_for_mode(&config.tool_registry, config.run_mode);

    let mut recovery = RecoveryState::default();
    let mut compact_count: u32 = 0;
    let mut total_usage = TokenUsage::default();
    let mut iteration: u32 = 0;

    loop {
        // ── Guards ────────────────────────────────────────────────────────────
        if runtime.cancel.is_cancelled() {
            return Ok(TurnResult::build(
                messages,
                TurnStopReason::Cancelled,
                total_usage,
                iteration,
            ));
        }
        iteration += 1;
        config.metrics.record_turn();
        if iteration > config.max_iterations {
            return Ok(TurnResult::build(
                messages,
                TurnStopReason::MaxIterations,
                total_usage,
                iteration,
            ));
        }

        // ── Compression ───────────────────────────────────────────────────────
        config.compression.apply_budget(messages);
        let pressure = ctx.check_pressure(ctx.messages_for_api(messages));
        if pressure == ContextPressure::Overflow {
            return Ok(TurnResult::build(
                messages,
                TurnStopReason::ContextOverflow,
                total_usage,
                iteration,
            ));
        }
        if pressure >= ContextPressure::Compact {
            for event in config
                .compression
                .apply_pressure(messages, pressure, &mut compact_count)
                .await
            {
                let loop_event = match event {
                    StageEvent::Collapsed { folded_count } => LoopEvent::Collapsed { folded_count },
                    StageEvent::Compacted { pre_tokens } => {
                        config.metrics.record_compact();
                        LoopEvent::Compacted { pre_tokens }
                    }
                };
                runtime.bus.emit(loop_event);
            }
        }

        // ── Call LLM ──────────────────────────────────────────────────────────
        let provider_config = ProviderConfig {
            model: config.model.clone(),
            max_tokens: effective_max_tokens(&recovery),
            temperature: config.temperature,
            extensions: config.provider_extensions.clone(),
        };
        let api_messages = ctx.messages_for_api(messages);
        tracing::debug!(
            iteration,
            remaining = ctx.remaining_before_compact(api_messages),
            "context budget"
        );
        let stream = match config
            .provider
            .stream(
                &config.system_prompt,
                api_messages,
                &tool_defs,
                &provider_config,
            )
            .await
        {
            Ok(s) => s,
            Err(e) => {
                return Ok(TurnResult::build(
                    messages,
                    TurnStopReason::Error(e.to_string()),
                    total_usage,
                    iteration,
                ));
            }
        };

        // ── Process stream ────────────────────────────────────────────────────
        let mut executor = ToolExecutor::new(
            Arc::clone(&config.tool_registry),
            runtime.cancel.clone(),
            config.cwd.clone(),
        );
        let outcome = StreamProcessor::new(
            config,
            runtime,
            hooks,
            messages,
            &mut executor,
            &mut recovery,
        )
        .run(stream)
        .await;

        match outcome {
            StreamOutcome::Retry => continue,
            StreamOutcome::Fatal(reason) => {
                runtime.bus.emit(LoopEvent::TurnEnd {
                    stop_reason: reason.clone(),
                    usage: total_usage.clone(),
                });
                return Ok(TurnResult::build(messages, reason, total_usage, iteration));
            }
            StreamOutcome::Done {
                assistant_content,
                usage,
                stop_reason,
            } => {
                messages.push(Message::assistant(new_id(), assistant_content.clone()));
                total_usage.accumulate(&usage);
                config
                    .metrics
                    .record_tokens(usage.input_tokens, usage.output_tokens);

                // ── Error recovery (two-phase) ────────────────────────────────
                let action = error_recovery::evaluate(
                    &stop_reason,
                    usage.output_tokens,
                    &mut recovery,
                    None,
                );

                match &action {
                    RecoveryAction::InjectContinuation => {
                        messages.push(Message::user(new_id(), continuation_prompt()));
                    }
                    RecoveryAction::CompactAndRetry => {
                        let events = config
                            .compression
                            .apply_pressure(messages, ContextPressure::Compact, &mut compact_count)
                            .await;
                        if events.iter().any(|e| matches!(e, StageEvent::Compacted { .. })) {
                            config.metrics.record_compact();
                        }
                    }
                    RecoveryAction::WaitAndRetry { duration } => {
                        tokio::time::sleep(*duration).await;
                    }
                    RecoveryAction::Abort { .. } => config.metrics.record_error(),
                    _ => {}
                }
                match action.into_loop_control() {
                    LoopControl::Continue => {}
                    LoopControl::Retry => continue,
                    LoopControl::Return(reason) => {
                        return Ok(TurnResult::build(messages, reason, total_usage, iteration));
                    }
                }

                // ── Turn end (no tool_use) ─────────────────────────────────────
                if stop_reason == StopReason::EndTurn {
                    match finalize_turn(
                        config,
                        runtime,
                        hooks,
                        &assistant_content,
                        messages,
                        total_usage.clone(),
                        iteration,
                    )
                    .await
                    {
                        Some(result) => return Ok(result),
                        None => continue,
                    }
                }

                // ── Collect tool results ──────────────────────────────────────
                collect_tool_results(executor, hooks, messages, runtime).await;
            }
        }
    }
}

enum DispatchOutcome {
    Submit(PendingTool),
    Blocked,
    RequiresAction { request_id: String },
}

// ============================================================================
// Stream processing
// ============================================================================

/// What the stream processing phase resolved to.
enum StreamOutcome {
    /// Stream ended normally. Carry out the accumulated turn data.
    Done {
        assistant_content: Vec<ContentBlock>,
        usage: TokenUsage,
        stop_reason: StopReason,
    },
    /// Transient stream error — outer loop should retry the API call.
    Retry,
    /// Unrecoverable — outer loop should return immediately with this reason.
    Fatal(TurnStopReason),
}

/// Partial tool call accumulated during streaming.
///
/// Named struct rather than `(String, String)` so the fields are self-documenting
/// and future additions (e.g. a progress channel) are a single-site change.
struct ToolAccumulator {
    name: String,
    json: String,
}

/// Stateful event-by-event consumer of one LLM stream.
///
/// `StreamProcessor::run(stream)` replaces the old 7-parameter `process_stream`
/// function. Owning the mutable state as fields makes each event handler a
/// focused one-liner instead of threading every variable through every call.
///
/// True streaming execution: tools are dispatched the moment `ToolUseEnd`
/// arrives, and `poll_completed()` harvests results between events so the
/// executor and the LLM run concurrently.
struct StreamProcessor<'a> {
    config: &'a LoopConfig,
    runtime: &'a LoopRuntime,
    hooks: &'a HookRunner,
    messages: &'a mut Vec<Message>,
    executor: &'a mut ToolExecutor,
    recovery: &'a mut RecoveryState,
    // Accumulated per-stream state
    content: Vec<ContentBlock>,
    buffers: HashMap<String, ToolAccumulator>,
    usage: TokenUsage,
    stop_reason: StopReason,
}

impl<'a> StreamProcessor<'a> {
    fn new(
        config: &'a LoopConfig,
        runtime: &'a LoopRuntime,
        hooks: &'a HookRunner,
        messages: &'a mut Vec<Message>,
        executor: &'a mut ToolExecutor,
        recovery: &'a mut RecoveryState,
    ) -> Self {
        Self {
            config,
            runtime,
            hooks,
            messages,
            executor,
            recovery,
            content: Vec::new(),
            buffers: HashMap::new(),
            usage: TokenUsage::default(),
            stop_reason: StopReason::EndTurn,
        }
    }

    async fn run(
        mut self,
        mut stream: Pin<Box<dyn Stream<Item = Result<StreamEvent, anyhow::Error>> + Send>>,
    ) -> StreamOutcome {
        while let Some(event) = stream.next().await {
            let event = match event {
                Ok(e) => e,
                Err(e) => return StreamOutcome::Fatal(TurnStopReason::Error(e.to_string())),
            };
            self.runtime.bus.emit(LoopEvent::Stream(event.clone()));

            if let Some(outcome) = self.handle(event).await {
                return outcome;
            }

            // Harvest tools that finished while the LLM was still streaming.
            for done in self.executor.poll_completed() {
                emit_tool_result(self.runtime, &done);
            }
        }

        StreamOutcome::Done {
            assistant_content: self.content,
            usage: self.usage,
            stop_reason: self.stop_reason,
        }
    }

    /// Route one event to the appropriate handler. Returns `Some(outcome)` only
    /// when the stream should be terminated early (error / retry).
    async fn handle(&mut self, event: StreamEvent) -> Option<StreamOutcome> {
        match event {
            StreamEvent::TextDelta { text } => self.on_text(text),
            StreamEvent::ThinkingDelta { text } => self.on_thinking(text),
            StreamEvent::ToolUseStart { id, name } => self.on_tool_start(id, name),
            StreamEvent::ToolInputDelta { id, json_chunk } => self.on_tool_delta(id, json_chunk),
            StreamEvent::ToolUseEnd { id } => self.on_tool_end(id).await,
            StreamEvent::MessageEnd { usage, stop_reason } => {
                self.usage = usage;
                self.stop_reason = stop_reason;
                None
            }
            StreamEvent::Error {
                message,
                is_retryable,
            } => {
                if is_retryable {
                    let kind = classify_stream_error(&message);
                    if let RecoveryAction::WaitAndRetry { duration } =
                        error_recovery::evaluate(&self.stop_reason, 0, self.recovery, Some(&kind))
                    {
                        tokio::time::sleep(duration).await;
                        return Some(StreamOutcome::Retry);
                    }
                }
                Some(StreamOutcome::Fatal(TurnStopReason::Error(message)))
            }
        }
    }

    fn on_text(&mut self, text: String) -> Option<StreamOutcome> {
        append_text(&mut self.content, text);
        None
    }

    fn on_thinking(&mut self, text: String) -> Option<StreamOutcome> {
        append_thinking(&mut self.content, text);
        None
    }

    fn on_tool_start(&mut self, id: String, name: String) -> Option<StreamOutcome> {
        self.buffers.insert(
            id,
            ToolAccumulator {
                name,
                json: String::new(),
            },
        );
        None
    }

    fn on_tool_delta(&mut self, id: String, json_chunk: String) -> Option<StreamOutcome> {
        if let Some(acc) = self.buffers.get_mut(&id) {
            acc.json.push_str(&json_chunk);
        }
        None
    }

    async fn on_tool_end(&mut self, id: String) -> Option<StreamOutcome> {
        if let Some(acc) = self.buffers.remove(&id) {
            let input = parse_tool_input(&acc.name, &acc.json);
            match dispatch_tool(
                id,
                acc.name,
                input,
                self.config,
                self.runtime,
                self.hooks,
                &mut self.content,
                self.messages,
            )
            .await
            {
                DispatchOutcome::Submit(pending) => {
                    self.runtime.bus.emit(LoopEvent::ToolStart {
                        id: pending.id.clone(),
                        name: pending.name.clone(),
                        input_preview: truncate_json_preview(&acc.json, 120),
                    });
                    self.executor.submit(pending);
                }
                DispatchOutcome::Blocked => {}
                DispatchOutcome::RequiresAction { request_id } => {
                    return Some(StreamOutcome::Fatal(TurnStopReason::RequiresAction {
                        request_id,
                    }));
                }
            }
        }
        None
    }
}

// ============================================================================
// Tool dispatch
// ============================================================================

/// Gate a single tool call through permission + PreToolUse hook, then submit.
///
/// Returns the next action to take for the completed tool call.
async fn dispatch_tool(
    id: String,
    name: String,
    input: serde_json::Value,
    config: &LoopConfig,
    runtime: &LoopRuntime,
    hooks: &HookRunner,
    assistant_content: &mut Vec<ContentBlock>,
    messages: &mut Vec<Message>,
) -> DispatchOutcome {
    // Permission check
    match config.permission.check(&name, &input) {
        PermissionDecision::Deny => {
            let reason = format!("Tool '{name}' is not permitted in this session.");
            runtime.bus.emit(LoopEvent::HookBlocked {
                hook_name: "permission".into(),
                reason: reason.clone(),
            });
            push_blocked_tool(id, name, input, reason, true, assistant_content, messages);
            config.metrics.record_error();
            return DispatchOutcome::Blocked;
        }
        PermissionDecision::Ask => {
            let request_id = new_id();
            let payload = serde_json::json!({
                "tool_name": name,
                "input": input,
            });
            runtime.set_pending_control(PendingControl {
                request_id: request_id.clone(),
                kind: "permission".into(),
                payload: payload.clone(),
            });
            runtime.bus.emit(LoopEvent::SessionStateChanged {
                state: "requires_action".into(),
            });
            runtime.bus.emit(LoopEvent::ControlRequest {
                request_id: request_id.clone(),
                kind: "permission".into(),
                payload,
            });
            return DispatchOutcome::RequiresAction { request_id };
        }
        PermissionDecision::Allow => {}
    }

    // PreToolUse hook
    let hook_event = HookEvent::PreToolUse {
        tool_name: name.clone(),
        input: input.clone(),
    };
    if let HookDecision::Block { reason } = hooks.run(&hook_event).await {
        runtime.bus.emit(LoopEvent::HookBlocked {
            hook_name: "pre_tool".into(),
            reason: reason.clone(),
        });
        push_blocked_tool(id, name, input, reason, true, assistant_content, messages);
        return DispatchOutcome::Blocked;
    }

    assistant_content.push(ContentBlock::ToolUse {
        id: id.clone(),
        name: name.clone(),
        input: input.clone(),
    });
    let mut message_history = messages.clone();
    message_history.push(Message::assistant(new_id(), assistant_content.clone()));
    config.metrics.record_tool_call();
    DispatchOutcome::Submit(PendingTool {
        id,
        name,
        input,
        message_history,
    })
}

/// Push a denied/blocked tool as an error tool_result so the LLM sees why.
fn push_blocked_tool(
    id: String,
    name: String,
    input: serde_json::Value,
    reason: String,
    is_error: bool,
    assistant_content: &mut Vec<ContentBlock>,
    messages: &mut Vec<Message>,
) {
    assistant_content.push(ContentBlock::ToolUse {
        id: id.clone(),
        name,
        input,
    });
    let result = ContentBlock::ToolResult {
        tool_use_id: id,
        content: reason,
        is_error,
    };
    messages.push(Message::assistant(new_id(), assistant_content.clone()));
    messages.push(Message::tool_results(new_id(), vec![result]));
}

// ============================================================================
// Turn finalization
// ============================================================================

/// Called when the LLM returned EndTurn (no tool_use).
///
/// Runs PreComplete hooks, then either:
/// - Returns `Some(result)` → the turn is done, return to caller
/// - Returns `None`         → hook forced a retry, outer loop should `continue`
async fn finalize_turn(
    config: &LoopConfig,
    runtime: &LoopRuntime,
    hooks: &HookRunner,
    assistant_content: &[ContentBlock],
    messages: &mut Vec<Message>,
    total_usage: TokenUsage,
    iteration: u32,
) -> Option<TurnResult> {
    let final_text = extract_text(assistant_content);

    if hooks.has_pre_complete_hooks() {
        let hook_event = HookEvent::PreComplete {
            assistant_text: final_text.clone(),
        };
        if let HookDecision::Block { reason } = hooks.run(&hook_event).await {
            runtime.bus.emit(LoopEvent::HookBlocked {
                hook_name: "pre_complete".into(),
                reason: reason.clone(),
            });
            messages.push(Message::user(new_id(), reason));
            return None; // LLM self-corrects → loop continues
        }
    }

    let stop_reason = if config.run_mode == RunMode::Planning {
        let request_id = new_id();
        let plan = crate::kernel::xml::plan(&final_text);
        let payload = serde_json::json!({
            "plan": plan,
            "steps": [],
        });
        runtime.set_pending_control(PendingControl {
            request_id: request_id.clone(),
            kind: "plan_review".into(),
            payload: payload.clone(),
        });
        runtime.bus.emit(LoopEvent::SessionStateChanged {
            state: "requires_action".into(),
        });
        runtime.bus.emit(LoopEvent::ControlRequest {
            request_id,
            kind: "plan_review".into(),
            payload,
        });
        TurnStopReason::PlanReview { plan }
    } else {
        TurnStopReason::Completed
    };

    runtime.bus.emit(LoopEvent::TurnEnd {
        stop_reason: stop_reason.clone(),
        usage: total_usage.clone(),
    });
    if !matches!(stop_reason, TurnStopReason::PlanReview { .. }) {
        runtime.bus.emit(LoopEvent::SessionStateChanged {
            state: "idle".into(),
        });
    }
    Some(TurnResult::build(
        messages,
        stop_reason,
        total_usage,
        iteration,
    ))
}

// ============================================================================
// Tool result collection
// ============================================================================

/// Await any still-running tools, run PostToolUse hooks, push results to messages.
async fn collect_tool_results(
    executor: ToolExecutor,
    hooks: &HookRunner,
    messages: &mut Vec<Message>,
    runtime: &LoopRuntime,
) {
    tracing::debug!(
        has_pending = executor.has_pending(),
        "collecting tool results"
    );
    let results = executor.collect_remaining().await;
    let mut result_blocks: Vec<ContentBlock> = Vec::new();

    for result in &results {
        // PostToolUse — audit only, never blocks.
        hooks
            .run(&HookEvent::PostToolUse {
                tool_name: result.name.clone(),
                input: serde_json::Value::Null,
                output: result.output.clone(),
            })
            .await;

        emit_tool_result(runtime, result);
        result_blocks.push(result.to_content_block());

        for extra in &result.output.extra_messages {
            messages.push(extra.clone());
        }
        for artifact in &result.output.artifacts {
            tracing::debug!(
                tool = result.name,
                kind = artifact.kind,
                title = artifact.title,
                "artifact produced"
            );
        }
    }

    messages.push(Message::tool_results(new_id(), result_blocks));
}

// ============================================================================
// Utilities
// ============================================================================

/// Emit a ToolResult loop event for a completed tool.
fn emit_tool_result(runtime: &LoopRuntime, done: &CompletedTool) {
    if done.name == "MemoryWrite" && !done.output.is_error {
        runtime.stats.record_memory_write();
    }
    runtime.bus.emit(LoopEvent::ToolResult {
        id: done.id.clone(),
        name: done.name.clone(),
        content: done.output.content.clone(),
        is_error: done.output.is_error,
    });
}

/// Accumulate a TextDelta into the last Text block, or push a new one.
fn append_text(content: &mut Vec<ContentBlock>, text: String) {
    match content.last_mut() {
        Some(ContentBlock::Text { text: existing }) => existing.push_str(&text),
        _ => content.push(ContentBlock::Text { text }),
    }
}

/// Accumulate a ThinkingDelta into the last Thinking block, or push a new one.
fn append_thinking(content: &mut Vec<ContentBlock>, text: String) {
    match content.last_mut() {
        Some(ContentBlock::Thinking { text: existing }) => existing.push_str(&text),
        _ => content.push(ContentBlock::Thinking { text }),
    }
}

/// Parse tool input JSON, returning Null on malformed input (LLM self-corrects).
fn parse_tool_input(tool_name: &str, json_str: &str) -> serde_json::Value {
    serde_json::from_str(json_str).unwrap_or_else(|e| {
        tracing::warn!(tool = tool_name, error = %e, "malformed tool input JSON");
        serde_json::Value::Null
    })
}

/// Build a short preview for a completed tool input payload.
fn truncate_json_preview(json_str: &str, max_len: usize) -> String {
    let normalized = serde_json::from_str::<serde_json::Value>(json_str)
        .map(|v| v.to_string())
        .unwrap_or_else(|_| json_str.split_whitespace().collect::<String>());

    if normalized.len() <= max_len {
        normalized
    } else {
        format!("{}...", &normalized[..max_len.saturating_sub(3)])
    }
}

/// Concatenate all Text blocks in a content list.
fn extract_text(content: &[ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

/// Classify a streaming error string for recovery routing.
fn classify_stream_error(message: &str) -> ApiErrorKind {
    let lower = message.to_lowercase();
    if lower.contains("rate limit") || lower.contains("429") {
        ApiErrorKind::RateLimit {
            retry_after_seconds: None,
        }
    } else if lower.contains("500") || lower.contains("503") || lower.contains("server error") {
        ApiErrorKind::ServerError
    } else if lower.contains("413") || lower.contains("too long") || lower.contains("prompt") {
        ApiErrorKind::PromptTooLong
    } else {
        ApiErrorKind::Other {
            message: message.to_string(),
        }
    }
}

/// Generate a fresh UUID string for message IDs.
fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
