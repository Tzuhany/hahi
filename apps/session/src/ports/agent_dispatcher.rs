// ============================================================================
// AgentDispatcher — Outbound Port for Agent Execution
//
// Defines what the application layer needs from the Agent service.
// The concrete implementation (infra::grpc::agent_client::AgentClient)
// is wired in at startup; tests inject a mock.
//
// Placing this trait in ports/ upholds hexagonal architecture:
//   app/ depends on this trait, never on the gRPC impl.
// ============================================================================

use anyhow::Result;

use hahi_proto::agent_event::ControlResponse;

#[async_trait::async_trait]
pub trait AgentDispatcher: Send + Sync {
    /// Dispatch a user message to the agent for execution.
    ///
    /// Returns the agent instance ID that accepted the run.
    /// The agent begins executing immediately and will write events to
    /// the Redis Stream `results:{run_id}`.
    async fn dispatch(
        &self,
        thread_id: &str,
        run_id: &str,
        message_id: &str,
        user_id: &str,
        content: &str,
    ) -> Result<String>;

    /// Forward a control response (permission decision or plan review) to the agent.
    ///
    /// Returns the run ID being resumed.
    async fn resume_run(
        &self,
        thread_id: &str,
        run_id: &str,
        user_id: &str,
        control: ControlResponse,
    ) -> Result<String>;

    /// Cancel the active run for a thread.
    ///
    /// Returns `true` if a run was running and was cancelled.
    async fn cancel_run(&self, thread_id: &str) -> Result<bool>;

    /// Query the agent's execution state for a thread.
    ///
    /// Returns `(status_string, active_request_id)`.
    async fn get_run_status(&self, thread_id: &str) -> Result<(String, Option<String>)>;
}
