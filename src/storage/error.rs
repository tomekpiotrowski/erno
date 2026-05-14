use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("File not found: {0}")]
    NotFound(String),
    #[error("Upload failed: {0}")]
    UploadFailed(String),
    #[error("Download failed: {0}")]
    DownloadFailed(String),
    #[error("Delete failed: {0}")]
    DeleteFailed(String),
    #[error("URL generation failed: {0}")]
    UrlGenerationFailed(String),
    #[error("Database error: {0}")]
    DatabaseError(#[from] sea_orm::DbErr),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}
