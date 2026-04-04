// ============================================================================
// User and Project Instructions
//
// Instructions are user-authored rules that shape the agent's behavior.
// They're the cloud equivalent of Claude Code's CLAUDE.md files.
//
// Two levels:
//   User-level:    applies to all of a user's conversations
//                  "Always respond in Chinese." / "Prefer concise answers."
//
//   Project-level: applies within a specific project
//                  "Use camelCase." / "This project uses PostgreSQL, not MySQL."
//
// Instructions live below the cache boundary (per-user content).
// They're fetched from the user/project services via gRPC at turn start.
// ============================================================================

/// Format user-level instructions for prompt injection.
///
/// Returns None if the user has no instructions configured.
pub fn format_user_instructions(instructions: &str) -> Option<String> {
    let trimmed = instructions.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(format!(
        "# User Instructions\n\
         The user has configured the following instructions. Follow them:\n\n\
         {trimmed}"
    ))
}

/// Format project-level instructions for prompt injection.
///
/// Returns None if the project has no instructions configured.
pub fn format_project_instructions(instructions: &str) -> Option<String> {
    let trimmed = instructions.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(format!(
        "# Project Instructions\n\
         The following project-specific instructions apply:\n\n\
         {trimmed}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_instructions_empty() {
        assert!(format_user_instructions("").is_none());
        assert!(format_user_instructions("   ").is_none());
    }

    #[test]
    fn test_user_instructions_formatted() {
        let result = format_user_instructions("Always respond in Chinese.").unwrap();
        assert!(result.contains("# User Instructions"));
        assert!(result.contains("Always respond in Chinese."));
    }

    #[test]
    fn test_project_instructions_formatted() {
        let result = format_project_instructions("Use PostgreSQL, not MySQL.").unwrap();
        assert!(result.contains("# Project Instructions"));
        assert!(result.contains("Use PostgreSQL, not MySQL."));
    }
}
