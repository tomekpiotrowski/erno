use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use tokio::fs;

use super::{error::StorageError, service::StorageService};

#[derive(Clone)]
pub struct LocalStorage {
    root: PathBuf,
}

impl LocalStorage {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

#[async_trait]
impl StorageService for LocalStorage {
    async fn upload(&self, key: &str, data: Bytes, _content_type: Option<&str>) -> Result<(), StorageError> {
        let path = self.root.join(key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&path, &data).await?;
        Ok(())
    }

    async fn download(&self, key: &str) -> Result<Bytes, StorageError> {
        let path = self.root.join(key);
        let data = fs::read(&path).await
            .map_err(|_| StorageError::NotFound(key.to_string()))?;
        Ok(Bytes::from(data))
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        let path = self.root.join(key);
        fs::remove_file(&path).await
            .map_err(|_| StorageError::DeleteFailed(key.to_string()))?;
        Ok(())
    }

    async fn url(&self, key: &str, _expires_in: Duration) -> Result<String, StorageError> {
        Ok(format!("/storage/{}", key))
    }
}
