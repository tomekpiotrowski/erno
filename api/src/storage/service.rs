use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;

use super::error::StorageError;

#[async_trait]
pub trait StorageService: Send + Sync {
    async fn upload(&self, key: &str, data: Bytes, content_type: Option<&str>) -> Result<(), StorageError>;
    async fn download(&self, key: &str) -> Result<Bytes, StorageError>;
    async fn delete(&self, key: &str) -> Result<(), StorageError>;
    /// Returns a presigned URL (S3) or a path hint for local storage.
    async fn url(&self, key: &str, expires_in: Duration) -> Result<String, StorageError>;
}
