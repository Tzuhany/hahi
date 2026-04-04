// ============================================================================
// Error Types
//
// GatewayError maps all internal failure modes to HTTP responses.
// Every handler returns Result<T> which axum converts via IntoResponse.
// ============================================================================

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

pub type Result<T> = std::result::Result<T, GatewayError>;

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("unauthorized")]
    Unauthorized,

    #[error("{0}")]
    BadRequest(String),

    #[error("upstream error: {0}")]
    Upstream(#[from] tonic::Status),

    #[error("internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            GatewayError::Unauthorized => (StatusCode::UNAUTHORIZED, self.to_string()),
            GatewayError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            GatewayError::Upstream(status) => {
                let http = grpc_to_http(status.code());
                (http, status.message().to_string())
            }
            GatewayError::Internal(e) => {
                tracing::error!(error = %e, "internal gateway error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".to_string(),
                )
            }
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}

fn grpc_to_http(code: tonic::Code) -> StatusCode {
    match code {
        tonic::Code::NotFound => StatusCode::NOT_FOUND,
        tonic::Code::InvalidArgument | tonic::Code::FailedPrecondition => StatusCode::BAD_REQUEST,
        tonic::Code::Unauthenticated => StatusCode::UNAUTHORIZED,
        tonic::Code::PermissionDenied => StatusCode::FORBIDDEN,
        tonic::Code::AlreadyExists => StatusCode::CONFLICT,
        tonic::Code::ResourceExhausted => StatusCode::TOO_MANY_REQUESTS,
        tonic::Code::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
