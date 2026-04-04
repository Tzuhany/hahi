// ============================================================================
// JWT Authentication Middleware
//
// Validates Bearer tokens on every request. On success, extracts the user_id
// claim and inserts it as a request extension for handlers to read.
//
// Public endpoints (health check) are excluded in the router, not here.
// This middleware assumes it will only run on protected routes.
// ============================================================================

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{DecodingKey, Validation, decode};
use serde::Deserialize;

use crate::config::AppState;
use crate::error::GatewayError;

/// Claims extracted from a validated JWT.
#[derive(Clone, Debug, Deserialize)]
pub struct Claims {
    /// The authenticated user's ID (`sub` = JWT "subject" claim).
    pub sub: String,

    /// Token expiry timestamp (Unix seconds).
    ///
    /// Not used directly in application code — `jsonwebtoken::decode` validates
    /// expiry automatically via `Validation::default()`. The field must be
    /// present in the struct for the JWT library to deserialize it correctly.
    #[allow(dead_code)]
    pub exp: u64,
}

pub async fn jwt_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> std::result::Result<Response, GatewayError> {
    let token = extract_bearer(request.headers()).ok_or(GatewayError::Unauthorized)?;

    let claims = decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| GatewayError::Unauthorized)?
    .claims;

    request.extensions_mut().insert(claims);
    Ok(next.run(request).await)
}

fn extract_bearer(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}
