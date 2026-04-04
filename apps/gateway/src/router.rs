// ============================================================================
// Router
//
// Middleware pipeline (outermost → innermost):
//   RequestId → Tracing → Auth (protected routes only) → Handler
//
// Route groups:
//   /health                          — no auth
//   /v1/threads/**                   — JWT required
//   /v1/runs/{run_id}/**              — JWT required, status + SSE
// ============================================================================

use axum::{
    Router, middleware,
    routing::{delete, get, post},
};
use tower_http::trace::TraceLayer;

use crate::config::AppState;
use crate::handlers::{health, session};
use crate::middleware::{auth::jwt_auth, request_id::request_id};

pub fn build(state: AppState) -> Router {
    let protected = Router::new()
        // ── Threads ──────────────────────────────────────────────────────────
        .route("/v1/threads", post(session::threads::create_thread))
        .route("/v1/threads", get(session::threads::list_threads))
        .route("/v1/threads/{id}", get(session::threads::get_thread))
        .route("/v1/threads/{id}", delete(session::threads::delete_thread))
        .route(
            "/v1/threads/{id}/messages",
            get(session::threads::list_messages),
        )
        .route(
            "/v1/threads/{id}/messages",
            post(session::threads::send_message),
        )
        .route(
            "/v1/threads/{id}/runs/cancel",
            post(session::runs::cancel_run),
        )
        .route(
            "/v1/threads/{id}/runs/resume",
            post(session::runs::resume_run),
        )
        // ── Events ───────────────────────────────────────────────────────────
        .route(
            "/v1/runs/{run_id}/status",
            get(session::runs::get_run_status),
        )
        .route(
            "/v1/runs/{run_id}/events",
            get(session::events::stream_events),
        )
        .layer(middleware::from_fn_with_state(state.clone(), jwt_auth));

    Router::new()
        .route("/health", get(health::health))
        .merge(protected)
        .layer(middleware::from_fn(request_id))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
