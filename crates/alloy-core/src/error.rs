use std::path::PathBuf;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Network error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("File error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid data: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Archive error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("Download of {url} failed: {reason}")]
    Download { url: String, reason: String },

    #[error("Checksum mismatch for {}: expected {expected}, got {actual}", path.display())]
    HashMismatch { path: PathBuf, expected: String, actual: String },

    #[error("{0}")]
    Auth(#[from] crate::auth::AuthError),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Not supported yet: {0}")]
    Unsupported(String),

    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}

/// Errors cross the Tauri bridge as plain strings; the UI shows them in toasts.
impl serde::Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
