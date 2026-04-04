use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::kernel::r#loop::TurnStopReason;
use crate::runtime::assembler::RunOutput;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunStatus {
    Running,
    RequiresAction,
    Cancelled,
    Idle,
}

impl RunStatus {
    pub fn as_proto(self) -> &'static str {
        match self {
            RunStatus::Running => "running",
            RunStatus::RequiresAction => "requires_action",
            RunStatus::Cancelled => "cancelled",
            RunStatus::Idle => "idle",
        }
    }
}

#[derive(Clone, Default)]
pub struct RunRegistry {
    inner: Arc<RwLock<HashMap<String, RunState>>>,
}

#[derive(Clone)]
pub struct RunState {
    pub status: RunStatus,
    pub active_request_id: Option<String>,
    pub cancel: Option<CancellationToken>,
}

impl RunRegistry {
    pub async fn mark_running(&self, thread_id: &str, cancel: CancellationToken) {
        self.inner.write().await.insert(
            thread_id.to_string(),
            RunState {
                status: RunStatus::Running,
                active_request_id: None,
                cancel: Some(cancel),
            },
        );
    }

    pub async fn apply_output(&self, thread_id: &str, output: &RunOutput) {
        let status = match &output.stop_reason {
            TurnStopReason::RequiresAction { .. } | TurnStopReason::PlanReview { .. } => {
                RunStatus::RequiresAction
            }
            TurnStopReason::Cancelled => RunStatus::Cancelled,
            _ => RunStatus::Idle,
        };
        self.inner.write().await.insert(
            thread_id.to_string(),
            RunState {
                status,
                active_request_id: output
                    .pending_control
                    .as_ref()
                    .map(|pending| pending.request_id.clone()),
                cancel: None,
            },
        );
    }

    pub async fn mark_idle(&self, thread_id: &str) {
        self.inner.write().await.insert(
            thread_id.to_string(),
            RunState {
                status: RunStatus::Idle,
                active_request_id: None,
                cancel: None,
            },
        );
    }

    pub async fn get(&self, thread_id: &str) -> Option<RunState> {
        self.inner.read().await.get(thread_id).cloned()
    }

    pub async fn cancel(&self, thread_id: &str) -> bool {
        let cancel = {
            let mut inner = self.inner.write().await;
            let Some(state) = inner.get_mut(thread_id) else {
                return false;
            };
            state.status = RunStatus::Cancelled;
            state.cancel.clone()
        };
        if let Some(cancel) = cancel {
            cancel.cancel();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancel_marks_state_cancelled() {
        let registry = RunRegistry::default();
        let token = CancellationToken::new();
        registry.mark_running("thread-1", token).await;

        assert!(registry.cancel("thread-1").await);
        let state = registry.get("thread-1").await.expect("state must exist");
        assert_eq!(state.status, RunStatus::Cancelled);
    }
}
