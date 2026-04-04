use anyhow::Result;
use async_trait::async_trait;

use crate::domain::{Message, Run, RunId, RunStatus, Thread, ThreadId};

/// Persistence port for Thread aggregates.
#[async_trait]
pub trait ThreadRepo: Send + Sync {
    async fn insert(&self, thread: &Thread) -> Result<()>;
    async fn find_by_id(&self, id: &ThreadId) -> Result<Option<Thread>>;
    async fn list_by_user(&self, user_id: &str, limit: i64, offset: i64) -> Result<Vec<Thread>>;
    async fn delete(&self, id: &ThreadId) -> Result<()>;
}

/// Persistence port for Run aggregates.
#[async_trait]
pub trait RunRepo: Send + Sync {
    async fn insert(&self, run: &Run) -> Result<()>;
    async fn update(&self, run: &Run) -> Result<()>;
    async fn find_by_id(&self, id: &RunId) -> Result<Option<Run>>;
    async fn find_latest_by_thread(&self, thread_id: &ThreadId) -> Result<Option<Run>>;

    /// Find all runs in a given status — used for crash recovery on startup.
    async fn find_by_status(&self, status: &RunStatus) -> Result<Vec<Run>>;
}

/// Persistence port for Message aggregates.
#[async_trait]
pub trait MessageRepo: Send + Sync {
    async fn insert(&self, message: &Message) -> Result<()>;
    async fn list_by_thread(
        &self,
        thread_id: &ThreadId,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Message>>;
}
