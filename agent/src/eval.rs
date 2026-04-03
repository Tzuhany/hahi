use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;
use futures::stream;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::adapters::llm::{LlmProvider, ProviderConfig, ToolDefinition};
use crate::common::{Message, StopReason, StreamEvent, TokenUsage, ToolContext, ToolOutput};
use crate::kernel::hooks::HookRunner;
use crate::kernel::r#loop::{LoopConfigBuilder, LoopRuntime, TurnStopReason, run_loop};
use crate::kernel::permission::PermissionEvaluator;
use crate::systems::tools::definition::Tool;
use crate::systems::tools::registry::ToolRegistry;

pub struct EvalCase {
    pub tools: Vec<Arc<dyn Tool>>,
    pub streams: Vec<Vec<StreamEvent>>,
    pub permission: Arc<PermissionEvaluator>,
}

pub struct EvalResult {
    pub stop_reason: TurnStopReason,
    pub messages: Vec<Message>,
    pub pending_request_id: Option<String>,
}

pub async fn run_eval_case(case: EvalCase) -> Result<EvalResult> {
    let provider = Arc::new(ScriptedProvider::new(case.streams));
    let registry = Arc::new(ToolRegistry::new(case.tools));
    let config = LoopConfigBuilder::new(provider, registry, "test system prompt")
        .permission(case.permission)
        .max_iterations(4)
        .build();
    let (bus, _rx) = crate::kernel::event_bus::EventBus::new();
    let runtime = LoopRuntime::new(CancellationToken::new(), bus);
    let hooks = HookRunner::empty();
    let mut messages = vec![Message::user("msg-1", "run eval")];
    let result = run_loop(&config, &runtime, &mut messages, &hooks).await?;
    let pending_request_id = runtime
        .take_pending_control()
        .map(|pending| pending.request_id);

    Ok(EvalResult {
        stop_reason: result.stop_reason,
        messages: result.messages,
        pending_request_id,
    })
}

struct ScriptedProvider {
    streams: Mutex<VecDeque<Vec<StreamEvent>>>,
}

impl ScriptedProvider {
    fn new(streams: Vec<Vec<StreamEvent>>) -> Self {
        Self {
            streams: Mutex::new(streams.into()),
        }
    }
}

#[async_trait]
impl LlmProvider for ScriptedProvider {
    async fn stream(
        &self,
        _system_prompt: &str,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _config: &ProviderConfig,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = Result<StreamEvent, anyhow::Error>> + Send>>>
    {
        let stream = self
            .streams
            .lock()
            .expect("scripted provider mutex poisoned")
            .pop_front()
            .unwrap_or_else(|| {
                vec![StreamEvent::MessageEnd {
                    usage: TokenUsage::default(),
                    stop_reason: StopReason::EndTurn,
                }]
            });
        Ok(Box::pin(stream::iter(stream.into_iter().map(Ok))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::permission::{PermissionDecision, PermissionMode, PermissionRule};

    struct FakeTool {
        name: &'static str,
        schema: serde_json::Value,
    }

    #[async_trait]
    impl Tool for FakeTool {
        fn name(&self) -> &str {
            self.name
        }
        fn description(&self) -> &str {
            self.name
        }
        fn prompt(&self) -> String {
            self.name.to_string()
        }
        fn input_schema(&self) -> serde_json::Value {
            self.schema.clone()
        }
        async fn call(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolOutput {
            ToolOutput::success(format!("ok: {input}"))
        }
    }

    #[tokio::test]
    async fn eval_argument_validation_surfaces_schema_errors() {
        let result = run_eval_case(EvalCase {
            tools: vec![Arc::new(FakeTool {
                name: "Echo",
                schema: json!({
                    "type": "object",
                    "properties": { "count": { "type": "integer" } },
                    "required": ["count"]
                }),
            })],
            streams: vec![
                vec![
                    StreamEvent::ToolUseStart {
                        id: "t1".into(),
                        name: "Echo".into(),
                    },
                    StreamEvent::ToolInputDelta {
                        id: "t1".into(),
                        json_chunk: r#"{"count":"oops"}"#.into(),
                    },
                    StreamEvent::ToolUseEnd { id: "t1".into() },
                    StreamEvent::MessageEnd {
                        usage: TokenUsage::default(),
                        stop_reason: StopReason::ToolUse,
                    },
                ],
                vec![StreamEvent::MessageEnd {
                    usage: TokenUsage::default(),
                    stop_reason: StopReason::EndTurn,
                }],
            ],
            permission: Arc::new(PermissionEvaluator::auto()),
        })
        .await
        .expect("eval should succeed");

        let flattened = format!("{:?}", result.messages);
        assert!(flattened.contains("Invalid input for tool 'Echo'"));
    }

    #[tokio::test]
    async fn eval_permission_ask_becomes_requires_action() {
        let permission = PermissionEvaluator::new(
            PermissionMode::Auto,
            vec![PermissionRule {
                tool_pattern: "Danger".into(),
                decision: PermissionDecision::Ask,
            }],
        );
        let result = run_eval_case(EvalCase {
            tools: vec![Arc::new(FakeTool {
                name: "Danger",
                schema: json!({ "type": "object" }),
            })],
            streams: vec![vec![
                StreamEvent::ToolUseStart {
                    id: "t1".into(),
                    name: "Danger".into(),
                },
                StreamEvent::ToolInputDelta {
                    id: "t1".into(),
                    json_chunk: "{}".into(),
                },
                StreamEvent::ToolUseEnd { id: "t1".into() },
                StreamEvent::MessageEnd {
                    usage: TokenUsage::default(),
                    stop_reason: StopReason::ToolUse,
                },
            ]],
            permission: Arc::new(permission),
        })
        .await
        .expect("eval should succeed");

        assert!(matches!(
            result.stop_reason,
            TurnStopReason::RequiresAction { .. }
        ));
        assert!(result.pending_request_id.is_some());
    }

    #[tokio::test]
    async fn eval_multi_tool_turn_records_both_results() {
        let result = run_eval_case(EvalCase {
            tools: vec![
                Arc::new(FakeTool {
                    name: "One",
                    schema: json!({ "type": "object" }),
                }),
                Arc::new(FakeTool {
                    name: "Two",
                    schema: json!({ "type": "object" }),
                }),
            ],
            streams: vec![
                vec![
                    StreamEvent::ToolUseStart {
                        id: "t1".into(),
                        name: "One".into(),
                    },
                    StreamEvent::ToolInputDelta {
                        id: "t1".into(),
                        json_chunk: "{}".into(),
                    },
                    StreamEvent::ToolUseEnd { id: "t1".into() },
                    StreamEvent::ToolUseStart {
                        id: "t2".into(),
                        name: "Two".into(),
                    },
                    StreamEvent::ToolInputDelta {
                        id: "t2".into(),
                        json_chunk: "{}".into(),
                    },
                    StreamEvent::ToolUseEnd { id: "t2".into() },
                    StreamEvent::MessageEnd {
                        usage: TokenUsage::default(),
                        stop_reason: StopReason::ToolUse,
                    },
                ],
                vec![StreamEvent::MessageEnd {
                    usage: TokenUsage::default(),
                    stop_reason: StopReason::EndTurn,
                }],
            ],
            permission: Arc::new(PermissionEvaluator::auto()),
        })
        .await
        .expect("eval should succeed");

        let flattened = format!("{:?}", result.messages);
        assert!(flattened.contains("tool_use_id: \"t1\""));
        assert!(flattened.contains("tool_use_id: \"t2\""));
    }

    #[test]
    fn eval_tool_search_finds_deferred_tools() {
        struct DeferredTool;
        #[async_trait]
        impl Tool for DeferredTool {
            fn name(&self) -> &str {
                "DeferredCalendar"
            }
            fn description(&self) -> &str {
                "Create and inspect calendar events"
            }
            fn prompt(&self) -> String {
                String::new()
            }
            fn input_schema(&self) -> serde_json::Value {
                json!({})
            }
            fn should_defer(&self) -> bool {
                true
            }
            fn search_hint(&self) -> Option<&str> {
                Some("calendar scheduling")
            }
            async fn call(&self, _input: serde_json::Value, _ctx: &ToolContext) -> ToolOutput {
                ToolOutput::success("ok")
            }
        }

        let registry = ToolRegistry::new(vec![Arc::new(DeferredTool)]);
        let matches = registry.search("calendar", 5);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "DeferredCalendar");
    }
}
