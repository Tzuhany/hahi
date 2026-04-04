// ============================================================================
// XML Tags for LLM Message Formatting
//
// Following Claude Code's pattern: the LLM naturally understands XML structure.
// We use XML tags to create clear boundaries between different types of content
// injected into the conversation.
//
// Why XML tags instead of JSON or markdown?
//   - LLMs are trained on vast amounts of XML/HTML and parse it reliably
//   - Tags create unambiguous boundaries (no escaping issues like with markdown)
//   - Nesting is natural (a system-reminder can contain structured sub-content)
//   - Claude Code proved this pattern works at massive scale
//
// Convention:
//   - Tags are kebab-case: <system-reminder>, <task-notification>
//   - System-injected content always wrapped in <system-reminder>
//   - Sub-agent notifications use <task-notification> with structured children
//   - User content is NEVER wrapped — the absence of tags means "user said this"
// ============================================================================

/// Wrap content in a `<system-reminder>` tag.
///
/// Used for all framework-injected context that is NOT user input:
///   - Deferred tool announcements
///   - Skill listings
///   - Memory recalls
///   - Date/environment context
///   - Conditional rules
///
/// The LLM is instructed (in the system prompt) to treat these as
/// system-provided context, not as user instructions.
pub fn system_reminder(content: &str) -> String {
    format!("<system-reminder>\n{content}\n</system-reminder>")
}

/// Format a task notification with usage statistics.
pub fn task_notification_with_usage(
    task_id: &str,
    status: &str,
    summary: &str,
    total_tokens: u64,
    tool_uses: u32,
    duration_ms: u64,
) -> String {
    format!(
        "<task-notification>\n\
         <task-id>{task_id}</task-id>\n\
         <status>{status}</status>\n\
         <summary>{summary}</summary>\n\
         <usage>\n\
         tokens: {total_tokens}, tool_uses: {tool_uses}, duration_ms: {duration_ms}\n\
         </usage>\n\
         </task-notification>"
    )
}

/// Wrap content in a `<plan>` tag.
///
/// Used when the agent is in planning mode and presents its plan
/// for user review. The structured format helps the LLM distinguish
/// between "thinking aloud" and "proposing a formal plan."
pub fn plan(content: &str) -> String {
    format!("<plan>\n{content}\n</plan>")
}

/// Wrap content in a `<plan-feedback>` tag.
///
/// User's modification request when they respond to a plan with "modify".
/// Tells the LLM this is feedback on the previous plan, not a new task.
pub fn plan_feedback(feedback: &str) -> String {
    format!("<plan-feedback>\n{feedback}\n</plan-feedback>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_reminder_wrapping() {
        let result = system_reminder("Today's date is 2026-04-03.");
        assert!(result.starts_with("<system-reminder>"));
        assert!(result.ends_with("</system-reminder>"));
        assert!(result.contains("Today's date is 2026-04-03."));
    }

    #[test]
    fn test_task_notification_with_usage() {
        let result =
            task_notification_with_usage("agent_456", "failed", "Timeout", 15000, 8, 12000);
        assert!(result.contains("<status>failed</status>"));
        assert!(result.contains("tokens: 15000"));
        assert!(result.contains("tool_uses: 8"));
    }

    #[test]
    fn test_plan_wrapping() {
        let result = plan("## Step 1\nDo something");
        assert!(result.starts_with("<plan>"));
        assert!(result.ends_with("</plan>"));
    }
}
