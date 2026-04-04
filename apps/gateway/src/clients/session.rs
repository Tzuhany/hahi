// ============================================================================
// Session Service Client
//
// Wraps both services exposed by the session gRPC server.
// Cloned per request — tonic channels are cheap to clone (shared pool).
// ============================================================================

use tonic::transport::Channel;

use hahi_proto::chat::{
    CancelRunRequest, CancelRunResponse, CreateThreadRequest, DeleteThreadRequest,
    DeleteThreadResponse, GetRunStatusRequest, GetRunStatusResponse, GetThreadRequest,
    ListMessagesRequest, ListMessagesResponse, ListThreadsRequest, ListThreadsResponse,
    ResumeRunRequest, ResumeRunResponse, SendMessageRequest, SendMessageResponse,
    StreamEventsRequest, ThreadProto, agent_execution_service_client::AgentExecutionServiceClient,
    thread_service_client::ThreadServiceClient,
};
use hahi_proto::events::EventFrame;

use crate::error::{GatewayError, Result};

#[derive(Clone)]
pub struct SessionClient {
    channel: Channel,
}

impl SessionClient {
    pub fn new(channel: Channel) -> Self {
        Self { channel }
    }

    fn execution(&self) -> AgentExecutionServiceClient<Channel> {
        AgentExecutionServiceClient::new(self.channel.clone())
    }

    fn thread(&self) -> ThreadServiceClient<Channel> {
        ThreadServiceClient::new(self.channel.clone())
    }
}

// ── AgentExecutionService ─────────────────────────────────────────────────────

impl SessionClient {
    pub async fn send_message(&self, req: SendMessageRequest) -> Result<SendMessageResponse> {
        self.execution()
            .send_message(req)
            .await
            .map(|r| r.into_inner())
            .map_err(GatewayError::from)
    }

    pub async fn stream_events(
        &self,
        run_id: String,
        last_event_id: String,
    ) -> Result<tonic::codec::Streaming<EventFrame>> {
        self.execution()
            .stream_events(StreamEventsRequest {
                run_id,
                last_event_id,
            })
            .await
            .map(|r| r.into_inner())
            .map_err(GatewayError::from)
    }

    pub async fn resume_run(&self, req: ResumeRunRequest) -> Result<ResumeRunResponse> {
        self.execution()
            .resume_run(req)
            .await
            .map(|r| r.into_inner())
            .map_err(GatewayError::from)
    }

    pub async fn cancel_run(&self, req: CancelRunRequest) -> Result<CancelRunResponse> {
        self.execution()
            .cancel_run(req)
            .await
            .map(|r| r.into_inner())
            .map_err(GatewayError::from)
    }

    pub async fn get_run_status(&self, run_id: String) -> Result<GetRunStatusResponse> {
        self.execution()
            .get_run_status(GetRunStatusRequest { run_id })
            .await
            .map(|r| r.into_inner())
            .map_err(GatewayError::from)
    }
}

// ── ThreadService ─────────────────────────────────────────────────────────────

impl SessionClient {
    pub async fn create_thread(&self, req: CreateThreadRequest) -> Result<ThreadProto> {
        self.thread()
            .create_thread(req)
            .await
            .map(|r| r.into_inner())
            .map_err(GatewayError::from)
    }

    pub async fn get_thread(&self, thread_id: String) -> Result<ThreadProto> {
        self.thread()
            .get_thread(GetThreadRequest { thread_id })
            .await
            .map(|r| r.into_inner())
            .map_err(GatewayError::from)
    }

    pub async fn list_threads(&self, req: ListThreadsRequest) -> Result<ListThreadsResponse> {
        self.thread()
            .list_threads(req)
            .await
            .map(|r| r.into_inner())
            .map_err(GatewayError::from)
    }

    pub async fn delete_thread(&self, thread_id: String) -> Result<DeleteThreadResponse> {
        self.thread()
            .delete_thread(DeleteThreadRequest { thread_id })
            .await
            .map(|r| r.into_inner())
            .map_err(GatewayError::from)
    }

    pub async fn list_messages(&self, req: ListMessagesRequest) -> Result<ListMessagesResponse> {
        self.thread()
            .list_messages(req)
            .await
            .map(|r| r.into_inner())
            .map_err(GatewayError::from)
    }
}
