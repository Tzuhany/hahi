// ============================================================================
// Post-Run Reflection
//
// After a session ends, the agent can reflect on what happened and decide
// what to commit to long-term memory. This is separate from in-run writes:
//
//   In-run writes:  reactive, immediate ("user just corrected me → write now")
//   Post-run:       reflective, holistic ("looking at the full session, what
//                   patterns or decisions are worth keeping?")
//
// This module decides WHEN to reflect and what prompt to use.
// The actual LLM call is done by the runtime layer.
// The LLM responds by calling MemoryWrite and/or MemoryForget tools.
//
// Reflection is not free (one extra LLM call), so it's gated:
//   - Only if the session had enough turns to be worth reflecting on
//   - Only if enough time has passed since the last reflection
//   - Always if the LLM wrote memories during the run (indicates something notable)
// ============================================================================

use chrono::{DateTime, Duration, Utc};

use crate::systems::memory::inject::format_write_guidance;
use crate::systems::memory::types::SessionStats;

/// Minimum turns for a session to be considered worth reflecting on.
const MIN_TURNS_TO_REFLECT: u32 = 5;

/// Minimum hours between reflections (prevents excessive calls on short sessions).
const MIN_HOURS_BETWEEN_REFLECTIONS: i64 = 12;

/// Decision from should_reflect().
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReflectDecision {
    /// Run reflection after this session.
    Yes { reason: &'static str },
    /// Skip reflection.
    No { reason: &'static str },
}

impl ReflectDecision {
    pub fn should_reflect(&self) -> bool {
        matches!(self, ReflectDecision::Yes { .. })
    }
}

/// Decide whether post-run reflection should be triggered.
pub fn should_reflect(stats: &SessionStats) -> ReflectDecision {
    // Always reflect if the LLM wrote memories — something notable happened.
    if stats.memories_written_this_run > 0 {
        return ReflectDecision::Yes {
            reason: "LLM wrote memories during this run",
        };
    }

    // Skip if the session was too short to be worth reflecting on.
    if stats.turn_count < MIN_TURNS_TO_REFLECT {
        return ReflectDecision::No {
            reason: "session too short",
        };
    }

    // Skip if we reflected recently (rate-limit expensive reflection calls).
    if let Some(last) = stats.last_reflection_at {
        let since = Utc::now().signed_duration_since(last);
        if since < Duration::hours(MIN_HOURS_BETWEEN_REFLECTIONS) {
            return ReflectDecision::No {
                reason: "reflected recently",
            };
        }
    }

    ReflectDecision::Yes {
        reason: "sufficient turns since last reflection",
    }
}

/// Build the system prompt for a reflection run.
///
/// The reflection is a focused mini-run: the LLM is given the full conversation
/// history and asked only to write/forget memories. It has access to MemoryWrite
/// and MemoryForget tools. After one turn the run ends.
pub fn reflection_system_prompt(memory_index: &str) -> String {
    format!(
        "You are in memory reflection mode.\n\n\
         Review the conversation that just ended and decide what is worth \
         committing to long-term memory.\n\n\
         {guidance}\n\n\
         ## Current memory index\n\
         {memory_index}\n\n\
         ## Instructions\n\
         - Call MemoryWrite for anything that meets the criteria above.\n\
         - Call MemoryForget for any existing memory that is now outdated or wrong.\n\
         - If nothing is worth saving or removing, call no tools.\n\
         - Do not output any text — only tool calls.",
        guidance = format_write_guidance(),
        memory_index = if memory_index.is_empty() {
            "(no memories yet)"
        } else {
            memory_index
        },
    )
}

/// Build the user message for the reflection turn.
///
/// Presents the full conversation history as the input.
pub fn reflection_user_message(conversation_summary: &str) -> String {
    format!(
        "Here is the conversation that just ended:\n\n{}\n\n\
         What should be committed to long-term memory?",
        conversation_summary
    )
}

/// Timestamp to persist as last_reflection_at after a successful reflection.
pub fn now() -> DateTime<Utc> {
    Utc::now()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(turns: u32, written: u32, last_reflection_hours_ago: Option<i64>) -> SessionStats {
        SessionStats {
            turn_count: turns,
            memories_written_this_run: written,
            last_reflection_at: last_reflection_hours_ago.map(|h| Utc::now() - Duration::hours(h)),
        }
    }

    #[test]
    fn test_reflect_if_memories_written() {
        // Even 1 turn is enough if something was written.
        let decision = should_reflect(&stats(1, 1, None));
        assert!(decision.should_reflect());
    }

    #[test]
    fn test_no_reflect_short_session() {
        let decision = should_reflect(&stats(3, 0, None));
        assert!(!decision.should_reflect());
    }

    #[test]
    fn test_no_reflect_too_recent() {
        // Reflected 6 hours ago, minimum is 12.
        let decision = should_reflect(&stats(10, 0, Some(6)));
        assert!(!decision.should_reflect());
    }

    #[test]
    fn test_reflect_long_session_no_recent_reflection() {
        let decision = should_reflect(&stats(10, 0, Some(24)));
        assert!(decision.should_reflect());
    }

    #[test]
    fn test_reflect_long_session_never_reflected() {
        let decision = should_reflect(&stats(10, 0, None));
        assert!(decision.should_reflect());
    }

    #[test]
    fn test_reflection_prompt_contains_guidance() {
        let prompt = reflection_system_prompt("(no memories yet)");
        assert!(prompt.contains("Do NOT save"));
        assert!(prompt.contains("MemoryWrite"));
        assert!(prompt.contains("MemoryForget"));
        assert!(prompt.contains("only tool calls"));
    }
}
