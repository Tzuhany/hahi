// ============================================================================
// ThreadService — Thread and Message CRUD
//
// Implements the gRPC ThreadService: thread lifecycle and message history.
// All persistence is delegated to the ThreadRepo and MessageRepo ports.
// ============================================================================

use std::sync::Arc;

use tonic::{Request, Response, Status};

use hahi_proto::chat::{
    ChatMessage, CreateThreadRequest, DeleteThreadRequest, DeleteThreadResponse, GetThreadRequest,
    ListMessagesRequest, ListMessagesResponse, ListThreadsRequest, ListThreadsResponse,
    MessageRole as ProtoMessageRole, ThreadProto, thread_service_server::ThreadService,
};

use crate::domain::{Thread, ThreadId};
use crate::ports::repository::{MessageRepo, ThreadRepo};

/// State for the `ThreadService` gRPC implementation.
///
/// Cloned cheaply per request — all fields are `Arc`.
#[derive(Clone)]
pub struct ThreadServiceImpl {
    pub thread_repo: Arc<dyn ThreadRepo>,
    pub message_repo: Arc<dyn MessageRepo>,
}

#[tonic::async_trait]
impl ThreadService for ThreadServiceImpl {
    async fn create_thread(
        &self,
        request: Request<CreateThreadRequest>,
    ) -> Result<Response<ThreadProto>, Status> {
        let req = request.into_inner();
        let thread = Thread::new(req.user_id, req.title);
        self.thread_repo
            .insert(&thread)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(thread_to_proto(&thread)))
    }

    async fn get_thread(
        &self,
        request: Request<GetThreadRequest>,
    ) -> Result<Response<ThreadProto>, Status> {
        let req = request.into_inner();
        let id = ThreadId::from(req.thread_id);
        let thread = self
            .thread_repo
            .find_by_id(&id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("thread not found"))?;
        Ok(Response::new(thread_to_proto(&thread)))
    }

    async fn list_threads(
        &self,
        request: Request<ListThreadsRequest>,
    ) -> Result<Response<ListThreadsResponse>, Status> {
        let req = request.into_inner();
        let pagination = req.pagination.unwrap_or_default();
        let limit = pagination.per_page.max(1).min(100) as i64;
        let offset = ((pagination.page.max(1) - 1) * pagination.per_page.max(1)) as i64;

        let threads = self
            .thread_repo
            .list_by_user(&req.user_id, limit, offset)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(ListThreadsResponse {
            threads: threads.iter().map(thread_to_proto).collect(),
            meta: None,
        }))
    }

    async fn delete_thread(
        &self,
        request: Request<DeleteThreadRequest>,
    ) -> Result<Response<DeleteThreadResponse>, Status> {
        let id = ThreadId::from(request.into_inner().thread_id);
        self.thread_repo
            .delete(&id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(DeleteThreadResponse {}))
    }

    async fn list_messages(
        &self,
        request: Request<ListMessagesRequest>,
    ) -> Result<Response<ListMessagesResponse>, Status> {
        let req = request.into_inner();
        let pagination = req.pagination.unwrap_or_default();
        let limit = pagination.per_page.max(1).min(100) as i64;
        let offset = ((pagination.page.max(1) - 1) * pagination.per_page.max(1)) as i64;

        let messages = self
            .message_repo
            .list_by_thread(&ThreadId::from(req.thread_id), limit, offset)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(ListMessagesResponse {
            messages: messages
                .into_iter()
                .map(|m| ChatMessage {
                    id: m.id.to_string(),
                    role: domain_message_role_to_proto(&m.role) as i32,
                    content: m.content,
                    created_at: m.created_at.to_rfc3339(),
                })
                .collect(),
            meta: None,
        }))
    }
}

fn domain_message_role_to_proto(r: &crate::domain::message::MessageRole) -> ProtoMessageRole {
    match r {
        crate::domain::message::MessageRole::User => ProtoMessageRole::User,
        crate::domain::message::MessageRole::Assistant => ProtoMessageRole::Assistant,
    }
}

fn thread_to_proto(t: &Thread) -> ThreadProto {
    ThreadProto {
        id: t.id.to_string(),
        user_id: t.user_id.clone(),
        title: t.title.clone(),
        created_at: t.created_at.to_rfc3339(),
        updated_at: t.updated_at.to_rfc3339(),
    }
}
