use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Clone)]
pub struct User {
    pub id: u64,
    pub username: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateUserPayload {
    pub username: String,
}
