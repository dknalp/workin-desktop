use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct AppError {
    pub code: String,
    pub message: String,
}

impl AppError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into() }
    }
    pub fn auth(msg: impl Into<String>) -> Self { Self::new("AUTH_ERROR", msg) }
    pub fn network(msg: impl Into<String>) -> Self { Self::new("NETWORK_ERROR", msg) }
    pub fn upload(msg: impl Into<String>) -> Self { Self::new("UPLOAD_ERROR", msg) }
    pub fn download(msg: impl Into<String>) -> Self { Self::new("DOWNLOAD_ERROR", msg) }
    pub fn io(msg: impl Into<String>) -> Self { Self::new("IO_ERROR", msg) }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self { Self::new("INTERNAL_ERROR", e.to_string()) }
}
impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self { Self::network(e.to_string()) }
}
impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self { Self::io(e.to_string()) }
}
impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self { Self::new("PARSE_ERROR", e.to_string()) }
}

pub type Result<T> = std::result::Result<T, AppError>;
