// ============================================================================
// Request ID Middleware
//
// Injects a unique X-Request-ID into every request and echoes it in the
// response. Uses the client-provided value when present; generates a new
// UUID v4 otherwise.
//
// This enables distributed tracing correlation across gateway → session →
// agent without a full tracing infrastructure.
// ============================================================================

use axum::{extract::Request, http::HeaderValue, middleware::Next, response::Response};

const HEADER: &str = "x-request-id";

pub async fn request_id(mut request: Request, next: Next) -> Response {
    let id = request
        .headers()
        .get(HEADER)
        .and_then(|v| v.to_str().ok())
        .map(ToString::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    if let Ok(value) = HeaderValue::from_str(&id) {
        request.headers_mut().insert(HEADER, value.clone());
    }

    let mut response = next.run(request).await;

    if let Ok(value) = HeaderValue::from_str(&id) {
        response.headers_mut().insert(HEADER, value);
    }

    response
}
