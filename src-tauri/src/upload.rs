use crate::error::{AppError, Result};
use crate::state::AppState;
use bytes::Bytes;
use futures::stream::{self, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
use tauri::{Emitter, State};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

const SMALL_FILE_THRESHOLD: u64 = 10 * 1024 * 1024;
const CHUNK_SIZE: u64 = 5 * 1024 * 1024;
const PARALLEL_CHUNKS: usize = 4;
const MAX_RETRIES: u32 = 3;

#[derive(Debug, Serialize, Clone)]
pub struct UploadProgress {
    pub transfer_id: String,
    pub path: String,
    pub bytes_done: u64,
    pub total_bytes: u64,
    pub speed_bps: u64,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PresignPutResponse {
    url: String,
    key: String,
}

#[derive(Debug, Deserialize)]
struct MultipartStartResponse {
    upload_id: String,
    urls: Vec<String>,
    key: String,
}

#[derive(Debug, Serialize)]
struct CompletedPart {
    part_number: u32,
    etag: String,
}

#[tauri::command]
pub async fn cmd_upload_file(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    local_path: String,
    remote_path: String,
    transfer_id: String,
) -> std::result::Result<(), AppError> {
    let (token, server_url) = crate::auth::get_auth_token(&state)?;
    let path = Path::new(&local_path);

    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|e| AppError::io(format!("Cannot read file: {}", e)))?;
    let total_bytes = metadata.len();

    let app2 = app.clone();
    let tid2 = transfer_id.clone();
    let rp2 = remote_path.clone();
    let emit = move |bytes_done: u64, speed_bps: u64, status: &str, error: Option<String>| {
        let _ = app2.emit("upload_progress", UploadProgress {
            transfer_id: tid2.clone(),
            path: rp2.clone(),
            bytes_done,
            total_bytes,
            speed_bps,
            status: status.to_string(),
            error,
        });
    };

    emit(0, 0, "uploading", None);

    let client = Arc::new(Client::new());
    let result = if total_bytes < SMALL_FILE_THRESHOLD {
        upload_single(&client, &token, &server_url, path, &remote_path, total_bytes, &emit).await
    } else {
        upload_multipart(&client, &token, &server_url, path, &remote_path, total_bytes, &emit).await
    };

    match &result {
        Ok(_) => emit(total_bytes, 0, "complete", None),
        Err(e) => emit(0, 0, "error", Some(e.message.clone())),
    }
    result
}

async fn upload_single(
    client: &Client,
    token: &str,
    server_url: &str,
    path: &Path,
    remote_path: &str,
    total_bytes: u64,
    emit: &impl Fn(u64, u64, &str, Option<String>),
) -> Result<()> {
    let base = server_url.trim_end_matches('/');
    let presign_resp = client
        .get(format!("{}/api/v1/files/presign/put", base))
        .bearer_auth(token)
        .query(&[("path", remote_path), ("size", &total_bytes.to_string())])
        .send()
        .await?;

    if !presign_resp.status().is_success() {
        return Err(AppError::upload(format!("Presign failed: {}", presign_resp.status())));
    }
    let presign: PresignPutResponse = presign_resp.json().await.map_err(|e| AppError::upload(e.to_string()))?;

    let data = tokio::fs::read(path).await.map_err(|e| AppError::io(e.to_string()))?;
    let start = std::time::Instant::now();

    let put_resp = client.put(&presign.url).body(data).send().await?;
    if !put_resp.status().is_success() {
        return Err(AppError::upload(format!("R2 PUT failed: {}", put_resp.status())));
    }

    let elapsed = start.elapsed().as_secs_f64().max(0.001);
    let speed_bps = (total_bytes as f64 / elapsed) as u64;
    emit(total_bytes, speed_bps, "uploading", None);

    record_metadata(client, token, base, remote_path, total_bytes).await
}

async fn upload_multipart(
    client: &Arc<Client>,
    token: &str,
    server_url: &str,
    path: &Path,
    remote_path: &str,
    total_bytes: u64,
    emit: &impl Fn(u64, u64, &str, Option<String>),
) -> Result<()> {
    let base = server_url.trim_end_matches('/');
    let num_chunks = ((total_bytes + CHUNK_SIZE - 1) / CHUNK_SIZE) as usize;

    let start_resp = client
        .post(format!("{}/api/v1/files/presign/multipart/start", base))
        .bearer_auth(token)
        .json(&serde_json::json!({
            "path": remote_path,
            "size": total_bytes,
            "parts": num_chunks,
        }))
        .send()
        .await?;

    if !start_resp.status().is_success() {
        return Err(AppError::upload(format!("Multipart start failed: {}", start_resp.status())));
    }
    let mp: MultipartStartResponse = start_resp.json().await.map_err(|e| AppError::upload(e.to_string()))?;

    if mp.urls.len() != num_chunks {
        return Err(AppError::upload(format!("Expected {} URLs, got {}", num_chunks, mp.urls.len())));
    }

    let bytes_done = Arc::new(AtomicU64::new(0));
    let upload_start = std::time::Instant::now();
    let path_arc = Arc::new(path.to_path_buf());
    let key = mp.key.clone();
    let upload_id = mp.upload_id.clone();

    let chunks: Vec<(usize, String)> = mp.urls.into_iter().enumerate().collect();
    let results: Vec<std::result::Result<(usize, String, u64), AppError>> = stream::iter(chunks)
        .map(|(idx, url)| {
            let client = client.clone();
            let path_arc = path_arc.clone();
            let bytes_done = bytes_done.clone();
            async move {
                let offset = idx as u64 * CHUNK_SIZE;
                let chunk_len = (total_bytes - offset).min(CHUNK_SIZE) as usize;
                let chunk_data = read_chunk(&path_arc, offset, chunk_len).await?;
                let chunk_size = chunk_data.len() as u64;

                let mut last_err = AppError::upload("no attempts");
                for attempt in 0..MAX_RETRIES {
                    if attempt > 0 {
                        tokio::time::sleep(tokio::time::Duration::from_secs(1 << (attempt - 1))).await;
                    }
                    match put_chunk(&client, &url, chunk_data.clone()).await {
                        Ok(etag) => {
                            bytes_done.fetch_add(chunk_size, Ordering::Relaxed);
                            return Ok((idx, etag, chunk_size));
                        }
                        Err(e) => last_err = e,
                    }
                }
                Err(last_err)
            }
        })
        .buffer_unordered(PARALLEL_CHUNKS)
        .collect()
        .await;

    let mut parts: Vec<CompletedPart> = vec![CompletedPart { part_number: 0, etag: String::new() }; num_chunks];
    for result in results {
        match result {
            Ok((idx, etag, _)) => {
                let done = bytes_done.load(Ordering::Relaxed);
                let elapsed = upload_start.elapsed().as_secs_f64().max(0.001);
                emit(done, (done as f64 / elapsed) as u64, "uploading", None);
                parts[idx] = CompletedPart { part_number: (idx + 1) as u32, etag };
            }
            Err(e) => {
                let _ = client
                    .delete(format!("{}/api/v1/files/presign/multipart/abort", base))
                    .bearer_auth(token)
                    .json(&serde_json::json!({ "upload_id": upload_id, "key": key }))
                    .send()
                    .await;
                return Err(e);
            }
        }
    }

    let complete_resp = client
        .post(format!("{}/api/v1/files/presign/multipart/complete", base))
        .bearer_auth(token)
        .json(&serde_json::json!({ "upload_id": upload_id, "key": key, "parts": parts }))
        .send()
        .await?;

    if !complete_resp.status().is_success() {
        return Err(AppError::upload(format!("Complete failed: {}", complete_resp.status())));
    }

    record_metadata(client, token, base, remote_path, total_bytes).await
}

async fn read_chunk(path: &Path, offset: u64, len: usize) -> Result<Bytes> {
    let mut file = File::open(path).await.map_err(|e| AppError::io(e.to_string()))?;
    file.seek(std::io::SeekFrom::Start(offset)).await.map_err(|e| AppError::io(e.to_string()))?;
    let mut buf = vec![0u8; len];
    file.read_exact(&mut buf).await.map_err(|e| AppError::io(e.to_string()))?;
    Ok(Bytes::from(buf))
}

async fn put_chunk(client: &Client, url: &str, data: Bytes) -> Result<String> {
    let resp = client.put(url).body(data).send().await.map_err(|e| AppError::upload(e.to_string()))?;
    if resp.status().is_success() {
        let etag = resp.headers().get("ETag")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .trim_matches('"')
            .to_string();
        Ok(etag)
    } else {
        Err(AppError::upload(format!("Chunk {} failed", resp.status())))
    }
}

async fn record_metadata(client: &Client, token: &str, base: &str, path: &str, size: u64) -> Result<()> {
    let resp = client
        .post(format!("{}/api/v1/files", base))
        .bearer_auth(token)
        .json(&serde_json::json!({ "path": path, "size": size }))
        .send()
        .await?;
    if resp.status().is_success() || resp.status() == 409 {
        Ok(())
    } else {
        Err(AppError::upload(format!("Metadata record failed: {}", resp.status())))
    }
}

#[tauri::command]
pub async fn cmd_list_files(
    state: State<'_, AppState>,
    path: String,
) -> std::result::Result<serde_json::Value, AppError> {
    let (token, server_url) = crate::auth::get_auth_token(&state)?;
    let client = Client::new();
    let base = server_url.trim_end_matches('/');
    let resp = client
        .get(format!("{}/api/v1/files", base))
        .bearer_auth(&token)
        .query(&[("path", &path)])
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(AppError::network(format!("Files list failed: {}", resp.status())));
    }
    let json: serde_json::Value = resp.json().await.map_err(|e| AppError::network(e.to_string()))?;
    Ok(json)
}
