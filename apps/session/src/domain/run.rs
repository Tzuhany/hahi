// ============================================================================
// Run — Agent Execution State Machine
//
// A Run represents one complete agent turn: from user message dispatch
// through streaming execution to final message persistence.
//
// State transitions (only valid paths):
//
//   Pending
//     └─ start()       → Running
//
//   Running
//     ├─ accumulate()  → Running   (content grows as deltas arrive)
//     ├─ complete()    → Completing (agent stream ended, content ready)
//     ├─ fail()        → Failed
//     └─ interrupt()   → Interrupted
//
//   Completing
//     └─ finalize()    → Completed (message written to DB)
//
// Completing is a crash-recovery window: if session restarts between
// the agent finishing and the DB write, the run stays in Completing.
// On restart, session replays Redis Stream to reconstruct and re-finalize.
// ============================================================================

use chrono::{DateTime, Utc};

use crate::domain::error::DomainError;
use crate::domain::ids::{MessageId, RunId, ThreadId};

/// The lifecycle state of a Run.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Created, not yet dispatched to an Agent.
    Pending,
    /// Agent is executing. Content accumulates as deltas arrive.
    Running,
    /// Agent finished streaming. Message not yet persisted to DB.
    /// Crash-safe window — session can re-finalize on restart.
    Completing,
    /// Message persisted. Run is done.
    Completed,
    /// Agent returned an error or the run was cancelled.
    Failed,
    /// Session or Agent restarted mid-run. Can be retried.
    Interrupted,
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            RunStatus::Pending => "pending",
            RunStatus::Running => "running",
            RunStatus::Completing => "completing",
            RunStatus::Completed => "completed",
            RunStatus::Failed => "failed",
            RunStatus::Interrupted => "interrupted",
        };
        f.write_str(s)
    }
}

/// One agent execution cycle within a Thread.
#[derive(Debug, Clone)]
pub struct Run {
    pub id: RunId,
    pub thread_id: ThreadId,
    pub status: RunStatus,

    /// The agent instance currently executing this run, if any.
    pub agent_id: Option<String>,
    /// When the agent started executing.
    pub started_at: Option<DateTime<Utc>>,
    /// When the run reached a terminal state.
    pub ended_at: Option<DateTime<Utc>>,

    /// In-memory buffer: text accumulates here from TextDelta events.
    /// Persisted atomically when `complete()` → `finalize()` succeeds.
    pub accumulated_content: String,

    /// Set on Completed — the persisted assistant message.
    pub result_message_id: Option<MessageId>,
    /// Set on Failed.
    pub failure_reason: Option<String>,

    pub created_at: DateTime<Utc>,
}

impl Run {
    /// Create a new Run in Pending state.
    pub fn new(thread_id: ThreadId) -> Self {
        Self {
            id: RunId::new(),
            thread_id,
            status: RunStatus::Pending,
            agent_id: None,
            started_at: None,
            ended_at: None,
            accumulated_content: String::new(),
            result_message_id: None,
            failure_reason: None,
            created_at: Utc::now(),
        }
    }

    /// Pending → Running.
    pub fn start(mut self, agent_id: impl Into<String>) -> Result<Self, DomainError> {
        if self.status != RunStatus::Pending {
            return Err(DomainError::InvalidStateTransition {
                from: self.status.to_string(),
                to: "running",
            });
        }
        self.status = RunStatus::Running;
        self.agent_id = Some(agent_id.into());
        self.started_at = Some(Utc::now());
        Ok(self)
    }

    /// Running → Running (append a text delta to the in-memory buffer).
    pub fn accumulate(&mut self, delta: &str) -> Result<(), DomainError> {
        if self.status != RunStatus::Running {
            return Err(DomainError::InvalidStateTransition {
                from: self.status.to_string(),
                to: "running",
            });
        }
        self.accumulated_content.push_str(delta);
        Ok(())
    }

    /// Running → Completing.
    ///
    /// Returns the fully accumulated content so the caller can persist it.
    /// The run stays in Completing until `finalize()` succeeds.
    pub fn complete(mut self) -> Result<(Self, String), DomainError> {
        if self.status != RunStatus::Running {
            return Err(DomainError::InvalidStateTransition {
                from: self.status.to_string(),
                to: "completing",
            });
        }
        let content = std::mem::take(&mut self.accumulated_content);
        self.status = RunStatus::Completing;
        self.ended_at = Some(Utc::now());
        Ok((self, content))
    }

    /// Completing → Completed.
    ///
    /// Called after the assistant message has been persisted to DB.
    pub fn finalize(mut self, message_id: MessageId) -> Result<Self, DomainError> {
        if self.status != RunStatus::Completing {
            return Err(DomainError::InvalidStateTransition {
                from: self.status.to_string(),
                to: "completed",
            });
        }
        self.status = RunStatus::Completed;
        self.result_message_id = Some(message_id);
        Ok(self)
    }

    /// Any non-terminal state → Failed.
    pub fn fail(mut self, reason: impl Into<String>) -> Result<Self, DomainError> {
        match self.status {
            RunStatus::Completed | RunStatus::Failed => {
                return Err(DomainError::InvalidStateTransition {
                    from: self.status.to_string(),
                    to: "failed",
                });
            }
            _ => {}
        }
        self.status = RunStatus::Failed;
        self.failure_reason = Some(reason.into());
        self.ended_at = Some(Utc::now());
        Ok(self)
    }

    /// Any active state → Interrupted (session/agent restarted).
    pub fn interrupt(mut self) -> Result<Self, DomainError> {
        match self.status {
            RunStatus::Pending | RunStatus::Running | RunStatus::Completing => {
                self.status = RunStatus::Interrupted;
                self.ended_at = Some(Utc::now());
                Ok(self)
            }
            _ => Err(DomainError::InvalidStateTransition {
                from: self.status.to_string(),
                to: "interrupted",
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending_run() -> Run {
        Run::new(ThreadId::new())
    }

    #[test]
    fn test_full_happy_path() {
        let run = pending_run();
        let mut run = run.start("agent-1").unwrap();

        run.accumulate("Hello").unwrap();
        run.accumulate(", world").unwrap();

        let (run, content) = run.complete().unwrap();
        assert_eq!(content, "Hello, world");
        assert_eq!(run.status, RunStatus::Completing);

        let run = run.finalize(MessageId::new()).unwrap();
        assert_eq!(run.status, RunStatus::Completed);
        assert!(run.result_message_id.is_some());
    }

    #[test]
    fn test_invalid_transition_rejects() {
        let run = pending_run();
        // Can't complete a Pending run
        assert!(run.complete().is_err());
    }

    #[test]
    fn test_fail_from_running() {
        let run = pending_run().start("agent-1").unwrap();
        let run = run.fail("LLM rate limit").unwrap();
        assert_eq!(run.status, RunStatus::Failed);
    }

    #[test]
    fn test_interrupt_from_completing() {
        let run = pending_run().start("agent-1").unwrap();
        let (run, _) = run.complete().unwrap();
        let run = run.interrupt().unwrap();
        assert_eq!(run.status, RunStatus::Interrupted);
    }

    #[test]
    fn test_cannot_fail_completed_run() {
        let run = pending_run().start("agent-1").unwrap();
        let (run, _) = run.complete().unwrap();
        let run = run.finalize(MessageId::new()).unwrap();
        assert!(run.fail("late error").is_err());
    }
}
