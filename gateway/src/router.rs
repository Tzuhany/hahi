use axum::Router;

use crate::config::AppState;

pub fn build(state: AppState) -> Router {
    Router::new()
        // .merge(chat_routes())
        // .merge(user_routes())
        // .merge(project_routes())
        // .merge(billing_routes())
        // .merge(memory_routes())
        // .merge(skill_routes())
        // .layer(middleware::auth::layer())
        .with_state(state)
}
