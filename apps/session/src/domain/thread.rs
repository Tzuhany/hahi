// ============================================================================
// Thread — Persistent Conversation Container
// ============================================================================

use chrono::{DateTime, Utc};

use crate::domain::ids::ThreadId;

/// A persistent conversation container.
///
/// Owns the sequence of messages exchanged with the agent.
/// Multiple Runs can occur within a single Thread (one per user turn).
/// Thread metadata lives here; checkpoint data lives in the agent service.
#[derive(Debug, Clone)]
pub struct Thread {
    pub id: ThreadId,
    /// The user who owns this thread.
    pub user_id: String,
    /// Display title. May be empty — clients should render a fallback.
    pub title: String,
    pub created_at: DateTime<Utc>,
    /// Updated when a new message is added or the title changes.
    pub updated_at: DateTime<Utc>,
}

impl Thread {
    /// Create a new Thread for `user_id` with the given `title`.
    pub fn new(user_id: impl Into<String>, title: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: ThreadId::new(),
            user_id: user_id.into(),
            title: title.into(),
            created_at: now,
            updated_at: now,
        }
    }
}
