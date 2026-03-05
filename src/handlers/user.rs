use crate::models::user::{CreateUserPayload, User};
use crate::services::user as UserService;
use crate::state::SharedState;
use axum::{Json, extract::State};

pub async fn create_user(
    State(state): State<SharedState>,
    Json(payload): Json<CreateUserPayload>,
) -> Json<User> {
    let new_user = UserService::create_user(&state, payload);
    Json(new_user)
}

pub async fn list_users(State(state): State<SharedState>) -> Json<Vec<User>> {
    let users = UserService::get_all_users(&state);
    Json(users)
}
