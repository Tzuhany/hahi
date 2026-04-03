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

use crate::common::Message;
use crate::core::xml;
use crate::skill::loader::{SkillDef, SkillMode};

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
    _parent_messages: &[Message],
) -> Result<SkillResult> {
    match skill.mode {
        SkillMode::Inline => execute_inline(skill, args).await,
        SkillMode::Forked => execute_forked(skill, args).await,
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
async fn execute_forked(skill: &SkillDef, args: Option<&str>) -> Result<SkillResult> {
    let mut prompt = skill.load_prompt()?;
    if let Some(args) = args {
        prompt.push_str(&format!("\n\nArguments: {args}"));
    }

    // TODO: spawn sub-agent via multi/spawn.rs
    // For now, return the prompt as output.
    tracing::debug!(
        skill = skill.name,
        "forked skill execution (sub-agent not yet wired)"
    );

    Ok(SkillResult::Forked {
        output: format!(
            "Skill '{}' executed (sub-agent pending implementation)",
            skill.name
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::ContentBlock;
    use std::fs;
    use std::path::PathBuf;

    fn test_skill(mode: SkillMode) -> SkillDef {
        let prompt_path: PathBuf = std::env::temp_dir().join("test-skill-prompt.md");
        fs::write(&prompt_path, "Do the thing step by step.").unwrap();

        SkillDef::new("test-skill", "A test skill", "", mode, prompt_path)
    }

    #[tokio::test]
    async fn test_inline_execution_wraps_in_system_reminder() {
        let skill = test_skill(SkillMode::Inline);
        let result = execute_skill(&skill, None, &[]).await.unwrap();

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
        let result = execute_skill(&skill, Some("-m 'fix bug'"), &[])
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
        let result = execute_skill(&skill, None, &[]).await.unwrap();
        assert!(matches!(result, SkillResult::Forked { .. }));
    }
}
