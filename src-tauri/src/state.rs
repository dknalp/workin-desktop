use std::sync::Mutex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthState {
    pub server_url: String,
    pub user_email: String,
    pub user_name: String,
    pub user_id: String,
    pub is_authenticated: bool,
}

#[derive(Default)]
pub struct AppState {
    pub auth: Mutex<AuthState>,
}
