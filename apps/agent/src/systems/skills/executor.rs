// ============================================================================
// Skill Executor — Inline and Forked Execution
//
// When the LLM invokes a skill, the executor loads its full prompt and
// runs it in one of two modes:
//
//   Inline:  The skill's prompt is injected directly into the current
//            conversation as a system message. The LLM continues with
//            the skill's instructions in context. Simple, no overhead.
//
//   Forked:  The skill runs in a sub-agent (tokio::spawn) with its own
//            message history, tool set, and potentially a different model.
//            Results are returned to the parent as a tool_result.
//            Used for complex skills that need isolation.
//
// The LLM doesn't choose the mode — it's declared in the skill definition.
// When the LLM calls Skill("commit"), the executor looks up the mode
// and handles it transparently.
// ============================================================================

use anyhow::Result;
use futures::future::BoxFuture;
use std::sync::Arc;

use crate::common::Message;
use crate::kernel::xml;
use crate::systems::skills::loader::{SkillDef, SkillMode};

/// Opaque fork capability injected by the runtime when a skill may run in a
/// sub-agent.
pub type SkillForkFn =
    dyn Fn(String, Vec<Message>) -> BoxFuture<'static, anyhow::Result<String>> + Send + Sync;

/// Per-invocation context for skill execution.
#[derive(Clone)]
pub struct SkillExecutionContext<'a> {
    /// Parent messages used to seed a forked sub-agent.
    pub parent_messages: &'a [Message],
    /// Optional fork executor. Required for forked skills.
    pub fork: Option<Arc<SkillForkFn>>,
}

/// Result of executing a skill.
#[derive(Debug)]
pub enum SkillResult {
    /// Inline skill: messages to inject into the current conversation.
    /// The agent loop appends these and continues.
    Inline { messages: Vec<Message> },

    /// Forked skill: completed sub-agent output.
    /// Returned as a tool_result to the LLM.
    Forked { output: String },
}

/// Execute a skill.
///
/// Loads the full prompt from the skill definition and runs it
/// according to its declared mode (inline or forked).
pub async fn execute_skill(
    skill: &SkillDef,
    args: Option<&str>,
    ctx: SkillExecutionContext<'_>,
) -> Result<SkillResult> {
    match skill.mode {
        SkillMode::Inline => execute_inline(skill, args).await,
        SkillMode::Forked => execute_forked(skill, args, ctx).await,
    }
}

/// Execute a skill in inline mode.
///
/// The skill's full prompt is wrapped in a `<system-reminder>` and
/// injected as a user message. The LLM sees it as system context
/// and follows the instructions in its next response.
async fn execute_inline(skill: &SkillDef, args: Option<&str>) -> Result<SkillResult> {
    let mut prompt = skill.load_prompt()?;

    // If the user provided arguments, append them.
    if let Some(args) = args {
        prompt.push_str(&format!("\n\nArguments: {args}"));
    }

    let reminder = xml::system_reminder(&prompt);
    let message = Message::user(uuid::Uuid::new_v4().to_string(), reminder);

    Ok(SkillResult::Inline {
        messages: vec![message],
    })
}

/// Execute a skill in forked mode.
///
/// Spawns a sub-agent with the skill's prompt as its initial instruction.
/// The sub-agent runs independently and returns its output.
async fn execute_forked(
    skill: &SkillDef,
    args: Option<&str>,
    ctx: SkillExecutionContext<'_>,
) -> Result<SkillResult> {
    let mut prompt = skill.load_prompt()?;
    if let Some(args) = args {
        prompt.push_str(&format!("\n\nArguments: {args}"));
    }

    let fork = ctx
        .fork
        .ok_or_else(|| anyhow::anyhow!("forked skill '{}' requires a fork executor", skill.name))?;
    let output = fork(prompt, ctx.parent_messages.to_vec()).await?;

    Ok(SkillResult::Forked { output })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::ContentBlock;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn test_skill(mode: SkillMode) -> SkillDef {
        let prompt_path: PathBuf = std::env::temp_dir().join("test-skill-prompt.md");
        fs::write(&prompt_path, "Do the thing step by step.").unwrap();

        SkillDef::new("test-skill", "A test skill", "", mode, prompt_path)
    }

    #[tokio::test]
    async fn test_inline_execution_wraps_in_system_reminder() {
        let skill = test_skill(SkillMode::Inline);
        let result = execute_skill(
            &skill,
            None,
            SkillExecutionContext {
                parent_messages: &[],
                fork: None,
            },
        )
        .await
        .unwrap();

        match result {
            SkillResult::Inline { messages } => {
                assert_eq!(messages.len(), 1);
                match &messages[0].content[0] {
                    ContentBlock::Text { text } => {
                        assert!(text.contains("<system-reminder>"));
                        assert!(text.contains("Do the thing step by step."));
                    }
                    _ => panic!("expected text content"),
                }
            }
            _ => panic!("expected inline result"),
        }
    }

    #[tokio::test]
    async fn test_inline_with_args() {
        let skill = test_skill(SkillMode::Inline);
        let result = execute_skill(
            &skill,
            Some("-m 'fix bug'"),
            SkillExecutionContext {
                parent_messages: &[],
                fork: None,
            },
        )
        .await
        .unwrap();

        match result {
            SkillResult::Inline { messages } => match &messages[0].content[0] {
                ContentBlock::Text { text } => {
                    assert!(text.contains("Arguments: -m 'fix bug'"));
                }
                _ => panic!("expected text content"),
            },
            _ => panic!("expected inline result"),
        }
    }

    #[tokio::test]
    async fn test_forked_execution() {
        let skill = test_skill(SkillMode::Forked);
        let fork: Arc<SkillForkFn> =
            Arc::new(|prompt: String, _messages: Vec<Message>| Box::pin(async move { Ok(prompt) }));
        let result = execute_skill(
            &skill,
            None,
            SkillExecutionContext {
                parent_messages: &[],
                fork: Some(fork),
            },
        )
        .await
        .unwrap();
        match result {
            SkillResult::Forked { output } => {
                assert!(output.contains("Do the thing step by step."));
            }
            _ => panic!("expected forked result"),
        }
    }

    #[tokio::test]
    async fn test_forked_execution_requires_executor() {
        let skill = test_skill(SkillMode::Forked);
        let err = execute_skill(
            &skill,
            None,
            SkillExecutionContext {
                parent_messages: &[],
                fork: None,
            },
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("requires a fork executor"));
    }
}
