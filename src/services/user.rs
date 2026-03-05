use crate::models::user::{CreateUserPayload, User};
use crate::state::SharedState;

pub fn create_user(state: &SharedState, payload: CreateUserPayload) -> User {
    let mut users = state.users.write().unwrap(); // Get write lock

    let new_user = User {
        id: (users.len() as u64) + 1,
        username: payload.username,
    };

    users.push(new_user.clone());
    new_user
}

pub fn get_all_users(state: &SharedState) -> Vec<User> {
    let users = state.users.read().unwrap(); // Get read lock
    users.clone()
}
