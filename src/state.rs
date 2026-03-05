use crate::models::user::User;
use std::sync::{Arc, RwLock};

// The shared state for your entire application
pub struct AppState {
    pub users: RwLock<Vec<User>>,
}

// We wrap it in an Arc so we can cheaply clone the reference across threads
pub type SharedState = Arc<AppState>;

impl AppState {
    pub fn new() -> SharedState {
        Arc::new(AppState {
            users: RwLock::new(Vec::new()),
        })
    }
}
