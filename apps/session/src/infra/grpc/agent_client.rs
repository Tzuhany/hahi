// ============================================================================
// Agent gRPC Client — AgentDispatcher Implementation
//
// Bridges the application layer's AgentDispatcher port to the real Agent
// gRPC service. The key design decision here:
//
//   dispatch() fires the gRPC call in a background task and returns
//   immediately. This is intentional — it lets run_lifecycle subscribe
//   to the Redis Stream before the agent starts writing events, enabling
//   true real-time streaming for connected clients.
//
//   The session service owns the run lifecycle. The agent is a pure
//   executor: it receives a message, runs the LLM loop, writes events
//   to Redis Stream, and exits. It does not know about Run records.
// ============================================================================

use anyhow::{Context, Result};
use async_trait::async_trait;
use tonic::transport::Channel;

use hahi_proto::chat::{
    CancelRunRequest, GetRunStatusRequest, ResumeRunRequest, SendMessageRequest,
    agent_execution_service_client::AgentExecutionServiceClient,
};

use hahi_proto::agent_event::ControlResponse;

use crate::ports::agent_dispatcher::AgentDispatcher;

pub struct AgentClient {
    channel: Channel,
}

impl AgentClient {
    pub async fn connect(url: &str) -> Result<Self> {
        let channel = Channel::from_shared(url.to_string())
            .context("invalid agent URL")?
            .connect()
            .await
            .with_context(|| format!("failed to connect to agent at {url}"))?;
        Ok(Self { channel })
    }
}

#[async_trait]
impl AgentDispatcher for AgentClient {
    async fn dispatch(
        &self,
        thread_id: &str,
        run_id: &str,
        message_id: &str,
        user_id: &str,
        content: &str,
    ) -> Result<String> {
        let mut client = AgentExecutionServiceClient::new(self.channel.clone());

        let req = SendMessageRequest {
            thread_id: thread_id.to_string(),
            run_id: run_id.to_string(),
            user_id: user_id.to_string(),
            content: content.to_string(),
        };

        // Fire and forget: the agent blocks until the run completes,
        // so we spawn a task to avoid blocking the session call path.
        // The agent writes events to Redis; run_lifecycle reads them.
        let tid = thread_id.to_string();
        let mid = message_id.to_string();
        let rid = run_id.to_string();
        tokio::spawn(async move {
            if let Err(e) = client.send_message(req).await {
                tracing::error!(
                    thread_id = %tid,
                    run_id = %rid,
                    message_id = %mid,
                    error = %e,
                    "agent dispatch failed"
                );
            }
        });

        // Return run_id as the agent instance identifier.
        // The session service uses this to correlate the Redis Stream key
        // `results:{run_id}` with the run being tracked.
        Ok(run_id.to_string())
    }

    async fn resume_run(
        &self,
        thread_id: &str,
        run_id: &str,
        user_id: &str,
        control: ControlResponse,
    ) -> Result<String> {
        let mut client = AgentExecutionServiceClient::new(self.channel.clone());
        let req = ResumeRunRequest {
            thread_id: thread_id.to_string(),
            user_id: user_id.to_string(),
            control: Some(control),
            run_id: run_id.to_string(),
        };

        let tid = thread_id.to_string();
        let rid = run_id.to_string();
        tokio::spawn(async move {
            if let Err(error) = client.resume_run(req).await {
                tracing::error!(thread_id = %tid, run_id = %rid, error = %error, "agent resume failed");
            }
        });

        Ok(run_id.to_string())
    }

    async fn cancel_run(&self, thread_id: &str) -> Result<bool> {
        let mut client = AgentExecutionServiceClient::new(self.channel.clone());
        let response = client
            .cancel_run(CancelRunRequest {
                thread_id: thread_id.to_string(),
            })
            .await
            .context("failed to cancel agent run")?;
        Ok(response.into_inner().cancelled)
    }

    async fn get_run_status(&self, thread_id: &str) -> Result<(String, Option<String>)> {
        let mut client = AgentExecutionServiceClient::new(self.channel.clone());
        let response = client
            .get_run_status(GetRunStatusRequest {
                run_id: thread_id.to_string(),
            })
            .await
            .context("failed to fetch agent run status")?
            .into_inner();
        Ok((response.status, response.active_request_id))
    }
}
