use crate::handlers::user::{create_user, list_users};
use crate::state::SharedState;
use axum::{
    Router,
    routing::{get, post},
};

pub fn create_router(state: SharedState) -> Router {
    Router::new()
        .route("/users", post(create_user))
        .route("/users", get(list_users))
        .with_state(state) // Injects your in-memory database into the routes
}
