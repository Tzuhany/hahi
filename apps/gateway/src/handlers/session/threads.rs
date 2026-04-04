// ============================================================================
// Session HTTP Handlers — Threads and Messages
//
// REST endpoints for thread CRUD + send message.
// All handlers require a valid JWT (enforced by the auth middleware layer).
// ============================================================================

use axum::{
    Json,
    extract::{Extension, Path, Query, State},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use hahi_proto::chat::{
    CreateThreadRequest, ListMessagesRequest, ListThreadsRequest, MessageRole as ProtoMessageRole,
    SendMessageRequest,
};
use hahi_proto::common::Pagination;

use crate::config::AppState;
use crate::error::Result;
use crate::middleware::auth::Claims;

// ── Threads ───────────────────────────────────────────────────────────────────

/// POST /v1/threads request body.
#[derive(Deserialize)]
pub struct CreateThreadBody {
    /// Optional display title. The session service accepts an empty string
    /// and clients should apply their own default ("New Thread", etc.).
    pub title: Option<String>,
}

/// POST /v1/threads
pub async fn create_thread(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<CreateThreadBody>,
) -> Result<Json<Value>> {
    let thread = state
        .session
        .create_thread(CreateThreadRequest {
            user_id: claims.sub,
            title: body.title.unwrap_or_default(),
        })
        .await?;

    Ok(Json(thread_to_json(thread)))
}

/// GET /v1/threads/:id
pub async fn get_thread(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let thread = state.session.get_thread(id).await?;
    Ok(Json(thread_to_json(thread)))
}

/// Query parameters for paginated list endpoints.
#[derive(Deserialize)]
pub struct PaginationParams {
    /// 1-based page number. Defaults to 1.
    pub page: Option<u32>,
    /// Items per page. Defaults vary by endpoint (threads: 20, messages: 50).
    pub per_page: Option<u32>,
}

/// GET /v1/threads
pub async fn list_threads(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<Value>> {
    let resp = state
        .session
        .list_threads(ListThreadsRequest {
            user_id: claims.sub,
            pagination: Some(Pagination {
                page: params.page.unwrap_or(1) as i32,
                per_page: params.per_page.unwrap_or(20) as i32,
            }),
        })
        .await?;

    let threads: Vec<Value> = resp.threads.into_iter().map(thread_to_json).collect();

    Ok(Json(json!({ "threads": threads })))
}

/// DELETE /v1/threads/:id
pub async fn delete_thread(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    state.session.delete_thread(id).await?;
    Ok(Json(json!({ "deleted": true })))
}

// ── Messages ──────────────────────────────────────────────────────────────────

/// GET /v1/threads/:id/messages
pub async fn list_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<Value>> {
    let resp = state
        .session
        .list_messages(ListMessagesRequest {
            thread_id: id,
            pagination: Some(Pagination {
                page: params.page.unwrap_or(1) as i32,
                per_page: params.per_page.unwrap_or(50) as i32,
            }),
        })
        .await?;

    let messages: Vec<Value> = resp
        .messages
        .into_iter()
        .map(|m| {
            json!({
                "id": m.id,
                "role": proto_role_to_str(m.role),
                "content": m.content,
                "created_at": m.created_at,
            })
        })
        .collect();

    Ok(Json(json!({ "messages": messages })))
}

// ── Send message ──────────────────────────────────────────────────────────────

/// POST /v1/threads/:id/messages request body.
#[derive(Deserialize)]
pub struct SendMessageBody {
    pub content: String,
}

/// POST /v1/threads/:id/messages response.
///
/// The run executes asynchronously. Use `run_id` to stream events via
/// `GET /v1/runs/:run_id/events` or poll status via `GET /v1/runs/:run_id/status`.
#[derive(Serialize)]
pub struct SendMessageResp {
    pub message_id: String,
    pub run_id: String,
}

/// POST /v1/threads/:id/messages
pub async fn send_message(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Json(body): Json<SendMessageBody>,
) -> Result<Json<SendMessageResp>> {
    let resp = state
        .session
        .send_message(SendMessageRequest {
            thread_id: id,
            user_id: claims.sub,
            content: body.content,
            // run_id is left empty — the session service assigns a fresh RunId.
        run_id: String::new(),
        })
        .await?;

    Ok(Json(SendMessageResp {
        message_id: resp.message_id,
        run_id: resp.run_id,
    }))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn proto_role_to_str(role: i32) -> &'static str {
    match ProtoMessageRole::try_from(role).unwrap_or(ProtoMessageRole::Unspecified) {
        ProtoMessageRole::User => "user",
        ProtoMessageRole::Assistant => "assistant",
        ProtoMessageRole::Unspecified => "unknown",
    }
}

fn thread_to_json(t: hahi_proto::chat::ThreadProto) -> Value {
    json!({
        "id": t.id,
        "user_id": t.user_id,
        "title": t.title,
        "created_at": t.created_at,
        "updated_at": t.updated_at,
    })
}
