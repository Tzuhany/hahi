// ============================================================================
// Streaming Concurrent Tool Executor
//
// Two key improvements over the naive "wait then execute" approach:
//
// 1. STREAMING EXECUTION:
//    Tools start executing AS SOON AS their input is complete (ToolUseEnd),
//    while the LLM may still be streaming additional tool calls or text.
//    submit() spawns a tokio task immediately — no waiting for the LLM to finish.
//
// 2. INPUT VALIDATION:
//    Before executing, the input is validated against the tool's JSON Schema.
//    Invalid input → immediate error result → LLM sees it and self-corrects.
//    No wasted time executing with bad arguments.
//
// Concurrency model:
//   - Tools marked is_concurrent() = true: spawn immediately, run in parallel
//   - Tools marked is_concurrent() = false: queued, run after all concurrent finish
//   - Validation failure: instant error, no spawn
//
// Timeline:
//   LLM streaming: ──token──token──ToolA──token──ToolB──token──done──
//   ToolA:                         ├────executing────────┤
//   ToolB:                                       ├──executing──┤
//   poll_completed():              ┄┄┄┄┄┄┄┄┄┄┄┄┄┄harvest┄┄┄┄┄harvest
//   collect_remaining():                                          ├─any left─┤
//
//   vs naive approach:
//   LLM streaming: ──token──token──ToolA──token──ToolB──token──done──
//   ToolA:                                                     ├──executing──┤
//   ToolB:                                                                  ├──┤
// ============================================================================

use std::collections::VecDeque;
use std::sync::Arc;

use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::common::{ContentBlock, ToolContext, ToolOutput, ToolProgress};
use crate::tool::definition::Tool;
use crate::tool::registry::ToolRegistry;

/// A tool invocation ready for execution.
#[derive(Debug)]
pub struct PendingTool {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
    pub message_history: Vec<crate::common::Message>,
}

/// A completed tool execution result.
#[derive(Debug)]
pub struct CompletedTool {
    pub id: String,
    pub name: String,
    pub output: ToolOutput,
}

/// A validated tool queued for sequential execution.
struct QueuedTool {
    id: String,
    name: String,
    input: serde_json::Value,
    message_history: Vec<crate::common::Message>,
    impl_: Arc<dyn Tool>,
}

impl CompletedTool {
    /// Convert to a ContentBlock for the message history.
    pub fn to_content_block(&self) -> ContentBlock {
        ContentBlock::ToolResult {
            tool_use_id: self.id.clone(),
            content: self.output.content.clone(),
            is_error: self.output.is_error,
        }
    }
}

/// Executes tools concurrently, with schema validation, while LLM streams.
///
/// Usage:
/// ```text
/// let mut executor = ToolExecutor::new(registry, cancel, cwd);
///
/// // During LLM streaming — tools start executing immediately:
/// executor.submit(PendingTool { id, name, input, message_history: vec![] });
/// executor.submit(PendingTool { id, name, input, message_history: vec![] });
///
/// // Also during streaming — harvest finished tools (non-blocking):
/// for done in executor.poll_completed() { ... }
///
/// // After LLM finishes — collect any still running:
/// let results = executor.collect_remaining().await;
/// ```
pub struct ToolExecutor {
    registry: Arc<ToolRegistry>,
    cancel: CancellationToken,
    cwd: std::path::PathBuf,

    /// JoinSet holds all spawned tool tasks.
    /// Each task returns (tool_use_id, name, output) so we can reconstruct
    /// CompletedTool even after the task is detached from the calling context.
    pending: JoinSet<(String, String, ToolOutput)>,

    /// Non-concurrent tools wait here until all parallel work finishes.
    sequential: VecDeque<QueuedTool>,
}

impl ToolExecutor {
    pub fn new(
        registry: Arc<ToolRegistry>,
        cancel: CancellationToken,
        cwd: std::path::PathBuf,
    ) -> Self {
        Self {
            registry,
            cancel,
            cwd,
            pending: JoinSet::new(),
            sequential: VecDeque::new(),
        }
    }

    /// Submit a tool for immediate execution.
    ///
    /// Called as soon as ToolUseEnd is received from the LLM stream.
    /// The tool starts running NOW — in parallel with the LLM's remaining output.
    ///
    /// Steps:
    ///   1. Look up the tool in the registry
    ///   2. Validate input against the tool's JSON Schema
    ///   3. If valid → JoinSet::spawn → runs concurrently
    ///   4. If invalid → instant error result (no spawn)
    ///   5. If unknown tool → instant error result
    pub fn submit(&mut self, tool: PendingTool) {
        let id = tool.id;
        let name = tool.name;
        let input = tool.input;
        let message_history = tool.message_history;

        let Some(impl_) = self.registry.get(&name) else {
            // Unknown tool → immediate error, captured in the set as a ready task.
            let output = ToolOutput::error(format!("unknown tool: {name}"));
            self.pending.spawn(async move { (id, name, output) });
            return;
        };

        // Validate input against the tool's JSON Schema.
        if let Err(errors) = validate_input(&input, &impl_.input_schema()) {
            let error_msg = format!("Invalid input for tool '{name}': {}", errors.join("; "));
            tracing::warn!(
                tool = name,
                errors = error_msg.as_str(),
                "tool input validation failed"
            );
            let output = ToolOutput::error(error_msg);
            self.pending.spawn(async move { (id, name, output) });
            return;
        }

        if impl_.is_concurrent() {
            self.spawn_tool(id, name, input, message_history, impl_);
        } else {
            self.sequential.push_back(QueuedTool {
                id,
                name,
                input,
                message_history,
                impl_,
            });
        }
    }

    /// Poll for tools that have already finished — non-blocking.
    ///
    /// Called DURING the LLM stream to yield results as they complete.
    /// Uses `JoinSet::try_join_next()`, which returns immediately with `None`
    /// if no task has finished yet — never waits.
    ///
    /// Returns all completed tools and removes them from the set.
    /// Remaining in-progress tools are collected later via `collect_remaining()`.
    pub fn poll_completed(&mut self) -> Vec<CompletedTool> {
        let mut results = Vec::new();
        while let Some(join_result) = self.pending.try_join_next() {
            match join_result {
                Ok((id, name, output)) => results.push(CompletedTool { id, name, output }),
                Err(e) => {
                    tracing::error!(error = %e, "tool task panicked during poll");
                    // Task panicked — we can't recover id/name from a JoinError,
                    // but this is a programming error (tools must not panic).
                    // The parent loop will notice the missing result when all
                    // tool_use_ids are reconciled with tool_results.
                }
            }
        }
        results
    }

    /// Collect ALL remaining results — waits for in-progress tools to finish.
    ///
    /// Called after LLM streaming ends. Between poll_completed() calls during
    /// streaming and this final collect, all tools are accounted for.
    pub async fn collect_remaining(mut self) -> Vec<CompletedTool> {
        let mut results = Vec::new();
        while let Some(join_result) = self.pending.join_next().await {
            match join_result {
                Ok((id, name, output)) => results.push(CompletedTool { id, name, output }),
                Err(e) => {
                    tracing::error!(error = %e, "tool task panicked during collect");
                }
            }
        }

        while let Some(tool) = self.sequential.pop_front() {
            let output = run_tool(
                tool.impl_,
                tool.input,
                tool.message_history,
                self.cancel.clone(),
                self.cwd.clone(),
            )
            .await;
            results.push(CompletedTool {
                id: tool.id,
                name: tool.name,
                output,
            });
        }

        results
    }

    /// Whether any tools are still pending (running or queued).
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty() || !self.sequential.is_empty()
    }

    fn spawn_tool(
        &mut self,
        id: String,
        name: String,
        input: serde_json::Value,
        message_history: Vec<crate::common::Message>,
        impl_: Arc<dyn Tool>,
    ) {
        let cancel = self.cancel.clone();
        let cwd = self.cwd.clone();
        self.pending.spawn(async move {
            let output = run_tool(impl_, input, message_history, cancel, cwd).await;
            (id, name, output)
        });
    }
}

async fn run_tool(
    impl_: Arc<dyn Tool>,
    input: serde_json::Value,
    message_history: Vec<crate::common::Message>,
    cancel: CancellationToken,
    cwd: std::path::PathBuf,
) -> ToolOutput {
    let ctx = ToolContext {
        cwd,
        cancel,
        message_history,
        on_progress: Box::new(|p: ToolProgress| {
            tracing::debug!(progress = p.message.as_str(), "tool progress");
        }),
    };
    impl_.call(input, &ctx).await
}

// ============================================================================
// Input Validation
// ============================================================================

/// Validate tool input against its JSON Schema.
///
/// Returns Ok(()) if valid, Err(Vec<String>) with error messages if invalid.
/// The LLM sees these error messages and can self-correct.
fn validate_input(
    input: &serde_json::Value,
    schema: &serde_json::Value,
) -> Result<(), Vec<String>> {
    let compiled = match jsonschema::validator_for(schema) {
        Ok(v) => v,
        Err(e) => {
            // Schema itself is invalid — log and allow (don't block on our bug).
            tracing::warn!(error = %e, "tool has invalid JSON Schema, skipping validation");
            return Ok(());
        }
    };

    let errors: Vec<_> = compiled.iter_errors(input).collect();
    if errors.is_empty() {
        return Ok(());
    }

    let messages: Vec<String> = errors
        .into_iter()
        .map(|e| {
            let path = e.instance_path.to_string();
            if path.is_empty() {
                e.to_string()
            } else {
                format!("{}: {}", path, e)
            }
        })
        .collect();

    Err(messages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use serde_json::json;
    use tokio::time::{Duration, sleep};

    struct RecordingTool {
        name: &'static str,
        concurrent: bool,
        delay_ms: u64,
        log: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl Tool for RecordingTool {
        fn name(&self) -> &str {
            self.name
        }
        fn description(&self) -> &str {
            self.name
        }
        fn prompt(&self) -> String {
            String::new()
        }
        fn input_schema(&self) -> serde_json::Value {
            json!({ "type": "object" })
        }
        fn is_concurrent(&self) -> bool {
            self.concurrent
        }

        async fn call(&self, _input: serde_json::Value, _ctx: &ToolContext) -> ToolOutput {
            if self.delay_ms > 0 {
                sleep(Duration::from_millis(self.delay_ms)).await;
            }
            self.log
                .lock()
                .expect("log mutex poisoned")
                .push(self.name.to_string());
            ToolOutput::success(self.name)
        }
    }

    #[test]
    fn test_validate_input_valid() {
        let schema = json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" }
            },
            "required": ["query"]
        });

        let input = json!({ "query": "hello" });
        assert!(validate_input(&input, &schema).is_ok());
    }

    #[test]
    fn test_validate_input_missing_required() {
        let schema = json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" }
            },
            "required": ["query"]
        });

        let input = json!({});
        let err = validate_input(&input, &schema).unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn test_validate_input_wrong_type() {
        let schema = json!({
            "type": "object",
            "properties": {
                "count": { "type": "integer" }
            }
        });

        let input = json!({ "count": "not a number" });
        let err = validate_input(&input, &schema).unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn test_validate_input_invalid_schema_passes() {
        // Bad schema shouldn't block execution.
        let schema = json!({ "type": "not_a_real_type" });
        let input = json!({});
        // Should either pass or log warning — never panic.
        let _ = validate_input(&input, &schema);
    }

    #[test]
    fn test_completed_tool_to_content_block() {
        let result = CompletedTool {
            id: "t1".into(),
            name: "WebSearch".into(),
            output: ToolOutput::success("found 3 results"),
        };
        let block = result.to_content_block();
        assert!(matches!(
            block,
            ContentBlock::ToolResult { tool_use_id, content, is_error }
            if tool_use_id == "t1" && content == "found 3 results" && !is_error
        ));
    }

    #[tokio::test]
    async fn test_non_concurrent_tools_wait_for_parallel_work() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let registry = Arc::new(ToolRegistry::new(vec![
            Arc::new(RecordingTool {
                name: "Concurrent",
                concurrent: true,
                delay_ms: 30,
                log: Arc::clone(&log),
            }),
            Arc::new(RecordingTool {
                name: "Sequential",
                concurrent: false,
                delay_ms: 0,
                log: Arc::clone(&log),
            }),
        ]));

        let mut executor = ToolExecutor::new(
            registry,
            CancellationToken::new(),
            std::path::PathBuf::from("/tmp"),
        );

        executor.submit(PendingTool {
            id: "c1".into(),
            name: "Concurrent".into(),
            input: json!({}),
            message_history: vec![],
        });
        executor.submit(PendingTool {
            id: "s1".into(),
            name: "Sequential".into(),
            input: json!({}),
            message_history: vec![],
        });

        sleep(Duration::from_millis(5)).await;
        assert!(executor.poll_completed().is_empty());

        let results = executor.collect_remaining().await;
        let names: Vec<_> = results.into_iter().map(|r| r.name).collect();
        assert_eq!(names, vec!["Concurrent", "Sequential"]);
        assert_eq!(
            log.lock().expect("log mutex poisoned").clone(),
            vec!["Concurrent".to_string(), "Sequential".to_string()],
        );
    }
}
