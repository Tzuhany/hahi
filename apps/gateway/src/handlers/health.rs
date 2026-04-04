use axum::{Json, http::StatusCode};
use serde_json::{Value, json};

/// GET /health — liveness probe. No auth required.
pub async fn health() -> (StatusCode, Json<Value>) {
    (StatusCode::OK, Json(json!({ "status": "ok" })))
}
