use crate::error::{AppError, Result};
use crate::state::AppState;
use keyring::Entry;
use serde::{Deserialize, Serialize};
use tauri::State;

const KEYRING_SERVICE: &str = "workin-desktop";
const TOKEN_KEY_SUFFIX: &str = ":access_token";
const REFRESH_KEY_SUFFIX: &str = ":refresh_token";
const SERVER_KEY: &str = "server_url";
const EMAIL_KEY: &str = "last_email";

#[derive(Debug, Deserialize)]
struct LoginResponse {
    access_token: String,
    refresh_token: Option<String>,
    user: UserInfo,
}

#[derive(Debug, Deserialize)]
struct RefreshResponse {
    access_token: String,
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct UserInfo {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SessionInfo {
    pub server_url: String,
    pub user_email: String,
    pub user_name: String,
    pub user_id: String,
}

fn token_entry(email: &str) -> std::result::Result<Entry, keyring::Error> {
    Entry::new(KEYRING_SERVICE, &format!("{}{}", email, TOKEN_KEY_SUFFIX))
}

fn refresh_entry(email: &str) -> std::result::Result<Entry, keyring::Error> {
    Entry::new(KEYRING_SERVICE, &format!("{}{}", email, REFRESH_KEY_SUFFIX))
}

fn meta_entry(key: &str) -> std::result::Result<Entry, keyring::Error> {
    Entry::new(KEYRING_SERVICE, key)
}

pub fn store_tokens(email: &str, access_token: &str, refresh_token: Option<&str>) -> Result<()> {
    token_entry(email)
        .map_err(|e| AppError::auth(e.to_string()))?
        .set_password(access_token)
        .map_err(|e| AppError::auth(e.to_string()))?;
    if let Some(rt) = refresh_token {
        refresh_entry(email)
            .map_err(|e| AppError::auth(e.to_string()))?
            .set_password(rt)
            .map_err(|e| AppError::auth(e.to_string()))?;
    }
    meta_entry(EMAIL_KEY)
        .map_err(|e| AppError::auth(e.to_string()))?
        .set_password(email)
        .map_err(|e| AppError::auth(e.to_string()))?;
    Ok(())
}

pub fn get_access_token(email: &str) -> Result<String> {
    token_entry(email)
        .map_err(|e| AppError::auth(e.to_string()))?
        .get_password()
        .map_err(|_| AppError::auth("No stored session — please sign in"))
}

pub fn get_refresh_token(email: &str) -> Result<String> {
    refresh_entry(email)
        .map_err(|e| AppError::auth(e.to_string()))?
        .get_password()
        .map_err(|_| AppError::auth("No refresh token stored"))
}

pub fn delete_tokens(email: &str) {
    let _ = token_entry(email).map(|e| e.delete_credential());
    let _ = refresh_entry(email).map(|e| e.delete_credential());
    let _ = meta_entry(EMAIL_KEY).map(|e| e.delete_credential());
    let _ = meta_entry(SERVER_KEY).map(|e| e.delete_credential());
}

pub fn get_stored_email() -> Option<String> {
    meta_entry(EMAIL_KEY).ok()?.get_password().ok()
}

pub fn get_stored_server_url() -> Option<String> {
    meta_entry(SERVER_KEY).ok()?.get_password().ok()
}

pub fn store_server_url(url: &str) -> Result<()> {
    meta_entry(SERVER_KEY)
        .map_err(|e| AppError::auth(e.to_string()))?
        .set_password(url)
        .map_err(|e| AppError::auth(e.to_string()))
}

// ── Tauri commands ──────────────────────────────────────────────────────────

#[tauri::command]
pub async fn cmd_login(
    server_url: String,
    email: String,
    password: String,
    state: State<'_, AppState>,
) -> std::result::Result<SessionInfo, AppError> {
    let client = reqwest::Client::new();
    let url = format!("{}/auth/login", server_url.trim_end_matches('/'));

    let resp = client
        .post(&url)
        .json(&serde_json::json!({ "email": email, "password": password }))
        .send()
        .await
        .map_err(|e| AppError::network(format!("Cannot reach server: {}", e)))?;

    if resp.status() == 401 || resp.status() == 400 {
        return Err(AppError::auth("Invalid email or password"));
    }
    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        return Err(AppError::auth(format!("Server returned {}", status)));
    }

    let body: LoginResponse = resp.json().await.map_err(|e| AppError::auth(e.to_string()))?;

    store_tokens(&email, &body.access_token, body.refresh_token.as_deref())?;
    store_server_url(&server_url)?;

    let name = body.user.name.clone().unwrap_or_else(|| email.clone());
    {
        let mut auth = state.auth.lock().unwrap();
        auth.server_url = server_url.clone();
        auth.user_email = email.clone();
        auth.user_name = name.clone();
        auth.user_id = body.user.id.clone();
        auth.is_authenticated = true;
    }

    Ok(SessionInfo {
        server_url,
        user_email: email,
        user_name: name,
        user_id: body.user.id,
    })
}

#[tauri::command]
pub async fn cmd_logout(state: State<'_, AppState>) -> std::result::Result<(), AppError> {
    let email = {
        let auth = state.auth.lock().unwrap();
        auth.user_email.clone()
    };
    delete_tokens(&email);
    let mut auth = state.auth.lock().unwrap();
    *auth = Default::default();
    Ok(())
}

#[tauri::command]
pub async fn cmd_restore_session(
    state: State<'_, AppState>,
) -> std::result::Result<Option<SessionInfo>, AppError> {
    let email = match get_stored_email() {
        Some(e) => e,
        None => return Ok(None),
    };
    let server_url = get_stored_server_url().unwrap_or_else(|| "https://workin.kiwimi.co".into());

    // Verify token is readable (not expired — full expiry check requires decoding JWT)
    match get_access_token(&email) {
        Ok(token) => {
            // Quick health check: call /auth/me
            let client = reqwest::Client::new();
            let me_url = format!("{}/api/v1/me", server_url.trim_end_matches('/'));
            let resp = client
                .get(&me_url)
                .bearer_auth(&token)
                .send()
                .await;

            match resp {
                Ok(r) if r.status().is_success() => {
                    let user: serde_json::Value = r.json().await.unwrap_or_default();
                    let name = user["name"].as_str().unwrap_or(&email).to_string();
                    let id = user["id"].as_str().unwrap_or("").to_string();
                    {
                        let mut auth = state.auth.lock().unwrap();
                        auth.server_url = server_url.clone();
                        auth.user_email = email.clone();
                        auth.user_name = name.clone();
                        auth.user_id = id.clone();
                        auth.is_authenticated = true;
                    }
                    Ok(Some(SessionInfo { server_url, user_email: email, user_name: name, user_id: id }))
                }
                Ok(r) if r.status() == 401 => {
                    // Try refresh
                    match try_refresh(&server_url, &email).await {
                        Ok(info) => Ok(Some(info)),
                        Err(_) => Ok(None),
                    }
                }
                _ => Ok(None),
            }
        }
        Err(_) => Ok(None),
    }
}

pub async fn try_refresh(server_url: &str, email: &str) -> Result<SessionInfo> {
    let rt = get_refresh_token(email)?;
    let client = reqwest::Client::new();
    let url = format!("{}/auth/refresh", server_url.trim_end_matches('/'));
    let resp = client
        .post(&url)
        .json(&serde_json::json!({ "refresh_token": rt }))
        .send()
        .await
        .map_err(|e| AppError::network(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(AppError::auth("Session expired — please sign in again"));
    }
    let body: RefreshResponse = resp.json().await.map_err(|e| AppError::auth(e.to_string()))?;
    store_tokens(email, &body.access_token, body.refresh_token.as_deref())?;

    Ok(SessionInfo {
        server_url: server_url.to_string(),
        user_email: email.to_string(),
        user_name: email.to_string(),
        user_id: String::new(),
    })
}

pub fn get_auth_token(state: &AppState) -> Result<(String, String)> {
    let (email, server_url) = {
        let auth = state.auth.lock().unwrap();
        if !auth.is_authenticated {
            return Err(AppError::auth("Not authenticated"));
        }
        (auth.user_email.clone(), auth.server_url.clone())
    };
    let token = get_access_token(&email)?;
    Ok((token, server_url))
}
