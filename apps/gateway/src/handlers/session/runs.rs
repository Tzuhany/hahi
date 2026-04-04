// ============================================================================
// Run Control Handlers
//
// Status and control-plane endpoints for in-flight agent runs.
// These map thinly to the session service's execution RPCs.
// ============================================================================

use axum::{
    Json,
    extract::{Extension, Path, State},
};
use serde::Deserialize;
use serde_json::{Value, json};

use hahi_proto::agent_event::{
    ControlResponse, PermissionDecision, PlanDecision, control_response,
};
use hahi_proto::chat::{CancelRunRequest, ResumeRunRequest};

use crate::config::AppState;
use crate::error::{GatewayError, Result};
use crate::middleware::auth::Claims;

/// GET /v1/runs/:run_id/status
pub async fn get_run_status(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>> {
    let resp = state.session.get_run_status(run_id.clone()).await?;
    Ok(Json(json!({
        "run_id": run_id,
        "status": resp.status,
        "active_request_id": resp.active_request_id,
    })))
}

/// POST /v1/threads/:id/runs/cancel
pub async fn cancel_run(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
) -> Result<Json<Value>> {
    let response = state
        .session
        .cancel_run(CancelRunRequest { thread_id })
        .await?;
    Ok(Json(json!({ "cancelled": response.cancelled })))
}

/// POST /v1/threads/:id/runs/resume request body.
///
/// Exactly one of `permission` or `plan_decision` must be present.
/// `request_id` must match the `request_id` in the `control_requested` event
/// that paused the run — it prevents stale or duplicate resumes.
#[derive(Deserialize)]
pub struct ResumeRunBody {
    /// The specific run to resume. Optional — session resolves the active
    /// run for the thread if omitted.
    pub run_id: Option<String>,
    /// Correlation ID from the `control_requested` SSE event.
    pub request_id: String,
    /// Response to a tool-permission pause.
    pub permission: Option<PermissionDecisionBody>,
    /// Response to a plan-review pause.
    pub plan_decision: Option<PlanDecisionBody>,
}

/// User's decision on a tool-permission request.
#[derive(Deserialize)]
pub struct PermissionDecisionBody {
    /// `true` = approve tool execution, `false` = deny.
    pub allowed: bool,
}

/// User's decision on a plan-review request.
#[derive(Deserialize)]
pub struct PlanDecisionBody {
    /// One of: `"approve"`, `"modify"`, `"reject"`.
    pub action: String,
    /// Optional free-text feedback when `action` is `"modify"` or `"reject"`.
    pub feedback: Option<String>,
}

/// POST /v1/threads/:id/runs/resume
pub async fn resume_run(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(thread_id): Path<String>,
    Json(body): Json<ResumeRunBody>,
) -> Result<Json<Value>> {
    let run_id = body.run_id.clone().unwrap_or_default();
    let control = build_control_response(body)?;
    let response = state
        .session
        .resume_run(ResumeRunRequest {
            thread_id,
            user_id: claims.sub,
            control: Some(control),
            run_id,
        })
        .await?;

    Ok(Json(json!({ "run_id": response.run_id })))
}

/// Convert the HTTP request body into the proto `ControlResponse` message.
///
/// Enforces the invariant that exactly one decision type is present.
/// Returns `BadRequest` if both or neither are provided.
fn build_control_response(body: ResumeRunBody) -> Result<ControlResponse> {
    let response = match (body.permission, body.plan_decision) {
        (Some(permission), None) => control_response::Response::Permission(PermissionDecision {
            allowed: permission.allowed,
        }),
        (None, Some(plan)) => control_response::Response::PlanDecision(PlanDecision {
            action: plan.action,
            feedback: plan.feedback,
        }),
        _ => {
            return Err(GatewayError::BadRequest(
                "resume request must include exactly one of permission or plan_decision"
                    .to_string(),
            ));
        }
    };

    Ok(ControlResponse {
        request_id: body.request_id,
        response: Some(response),
    })
}
