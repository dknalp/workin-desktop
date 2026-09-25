use crate::error::{AppError, Result};
use crate::state::AppState;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::{Emitter, State};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

const BUFFER_SIZE: usize = 256 * 1024; // 256 KB — emit progress every this many bytes
const MAX_RETRIES: u32 = 3;

#[derive(Debug, Serialize, Clone)]
pub struct DownloadProgress {
    pub transfer_id: String,
    pub path: String,
    pub bytes_done: u64,
    pub total_bytes: u64,
    pub speed_bps: u64,
    pub status: String, // "downloading" | "complete" | "error"
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PresignDownloadResponse {
    url: String,
}

#[tauri::command]
pub async fn cmd_download_file(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    remote_path: String,
    local_path: String,
    transfer_id: String,
) -> std::result::Result<(), AppError> {
    let (token, server_url) = crate::auth::get_auth_token(&state)?;

    let emit = {
        let app = app.clone();
        let tid = transfer_id.clone();
        let rp = remote_path.clone();
        move |bytes_done: u64, total_bytes: u64, speed_bps: u64, status: &str, error: Option<String>| {
            let _ = app.emit("download_progress", DownloadProgress {
                transfer_id: tid.clone(),
                path: rp.clone(),
                bytes_done,
                total_bytes,
                speed_bps,
                status: status.to_string(),
                error,
            });
        }
    };

    emit(0, 0, 0, "downloading", None);

    let mut last_err = AppError::download("no attempts");
    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            tokio::time::sleep(tokio::time::Duration::from_secs(1 << (attempt - 1))).await;
        }
        match do_download(&token, &server_url, &remote_path, &local_path, &emit).await {
            Ok(_) => {
                return Ok(());
            }
            Err(e) => last_err = e,
        }
    }
    emit(0, 0, 0, "error", Some(last_err.message.clone()));
    Err(last_err)
}

async fn do_download(
    token: &str,
    server_url: &str,
    remote_path: &str,
    local_path: &str,
    emit: &impl Fn(u64, u64, u64, &str, Option<String>),
) -> Result<()> {
    let base = server_url.trim_end_matches('/');
    let client = Client::new();

    // Get presigned download URL
    let presign_resp = client
        .get(format!("{}/api/v1/files/presign/download", base))
        .bearer_auth(token)
        .query(&[("path", remote_path)])
        .send()
        .await?;

    if !presign_resp.status().is_success() {
        return Err(AppError::download(format!(
            "Failed to get download URL: {}",
            presign_resp.status()
        )));
    }
    let presign: PresignDownloadResponse = presign_resp
        .json()
        .await
        .map_err(|e| AppError::download(e.to_string()))?;

    // Stream bytes directly from R2 to disk
    let resp = client.get(&presign.url).send().await?;
    if !resp.status().is_success() {
        return Err(AppError::download(format!("R2 GET failed: {}", resp.status())));
    }

    let total_bytes = resp.content_length().unwrap_or(0);

    // Create parent directories if needed
    if let Some(parent) = Path::new(local_path).parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| AppError::io(e.to_string()))?;
    }

    let mut file = File::create(local_path)
        .await
        .map_err(|e| AppError::io(format!("Cannot create file: {}", e)))?;

    let mut stream = resp.bytes_stream();
    let mut bytes_done: u64 = 0;
    let mut buffer = Vec::with_capacity(BUFFER_SIZE);
    let start = std::time::Instant::now();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AppError::download(e.to_string()))?;
        buffer.extend_from_slice(&chunk);
        bytes_done += chunk.len() as u64;

        if buffer.len() >= BUFFER_SIZE {
            file.write_all(&buffer)
                .await
                .map_err(|e| AppError::io(e.to_string()))?;
            buffer.clear();

            let elapsed = start.elapsed().as_secs_f64().max(0.001);
            let speed_bps = (bytes_done as f64 / elapsed) as u64;
            emit(bytes_done, total_bytes, speed_bps, "downloading", None);
        }
    }

    // Flush remaining buffer
    if !buffer.is_empty() {
        file.write_all(&buffer)
            .await
            .map_err(|e| AppError::io(e.to_string()))?;
    }
    file.flush().await.map_err(|e| AppError::io(e.to_string()))?;

    let elapsed = start.elapsed().as_secs_f64().max(0.001);
    let speed_bps = (bytes_done as f64 / elapsed) as u64;
    emit(bytes_done, total_bytes, speed_bps, "complete", None);
    Ok(())
}

#[tauri::command]
pub async fn cmd_get_downloads_dir() -> std::result::Result<String, AppError> {
    let dir = dirs::download_dir()
        .or_else(dirs::home_dir)
        .ok_or_else(|| AppError::io("Cannot find downloads directory"))?;
    Ok(dir.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn cmd_create_folder(
    state: State<'_, AppState>,
    path: String,
) -> std::result::Result<(), AppError> {
    let (token, server_url) = crate::auth::get_auth_token(&state)?;
    let client = Client::new();
    let base = server_url.trim_end_matches('/');
    let resp = client
        .post(format!("{}/api/v1/files/folder", base))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "path": path }))
        .send()
        .await?;
    if resp.status().is_success() || resp.status() == 409 {
        Ok(())
    } else {
        Err(AppError::upload(format!("Create folder failed: {}", resp.status())))
    }
}

#[tauri::command]
pub async fn cmd_rename_file(
    state: State<'_, AppState>,
    old_path: String,
    new_path: String,
) -> std::result::Result<(), AppError> {
    let (token, server_url) = crate::auth::get_auth_token(&state)?;
    let client = Client::new();
    let base = server_url.trim_end_matches('/');
    let resp = client
        .patch(format!("{}/api/v1/files/rename", base))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "old_path": old_path, "new_path": new_path }))
        .send()
        .await?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(AppError::upload(format!("Rename failed: {}", resp.status())))
    }
}

#[tauri::command]
pub async fn cmd_delete_file(
    state: State<'_, AppState>,
    path: String,
) -> std::result::Result<(), AppError> {
    let (token, server_url) = crate::auth::get_auth_token(&state)?;
    let client = Client::new();
    let base = server_url.trim_end_matches('/');
    let resp = client
        .delete(format!("{}/api/v1/files", base))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "path": path }))
        .send()
        .await?;
    if resp.status().is_success() || resp.status() == 404 {
        Ok(())
    } else {
        Err(AppError::upload(format!("Delete failed: {}", resp.status())))
    }
}
