use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use bytes::Bytes;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    Set,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::config::{StorageBackend, StorageConfig};

pub mod error;
pub mod local;
pub mod models;
pub mod s3;
pub mod service;

pub use error::StorageError;
pub use models::file;
pub use models::file_attachment;
pub use service::StorageService;

use local::LocalStorage;
use s3::S3Storage;

/// File storage backend — local disk, S3-compatible (AWS, Digital Ocean Spaces, MinIO), or mock for tests.
#[derive(Clone)]
pub enum FileStorage {
    Local(LocalStorage),
    S3(S3Storage),
    Mock(Arc<Mutex<HashMap<String, Bytes>>>),
}

impl FileStorage {
    pub fn from_config(config: &StorageConfig) -> Self {
        match config.backend {
            StorageBackend::Local => {
                let path = config
                    .local_path
                    .clone()
                    .unwrap_or_else(|| "./storage".to_string());
                Self::Local(LocalStorage::new(path))
            }
            StorageBackend::S3 => {
                let s3_config = config
                    .s3
                    .as_ref()
                    .expect("storage.s3 config is required when storage.backend = \"s3\"");
                Self::S3(S3Storage::new(s3_config))
            }
        }
    }

    pub fn mock() -> Self {
        Self::Mock(Arc::new(Mutex::new(HashMap::new())))
    }

    /// Compute checksum, upload bytes to the backend, and insert a `files` row.
    ///
    /// On DB insert failure the uploaded bytes are deleted (best-effort) to avoid orphans.
    pub async fn store(
        &self,
        db: &DatabaseConnection,
        filename: impl Into<String>,
        content_type: Option<&str>,
        data: Bytes,
    ) -> Result<file::Model, StorageError> {
        let filename = filename.into();
        let id = Uuid::new_v4();
        let key = format!("{}/{}", &id.to_string()[..2], id);

        let checksum = {
            let mut hasher = Sha256::new();
            hasher.update(&data);
            STANDARD.encode(hasher.finalize())
        };

        let byte_size = data.len() as i64;

        self.upload_raw(&key, data, content_type).await?;

        let now = Utc::now().naive_utc();
        let active = file::ActiveModel {
            id: Set(id),
            key: Set(key.clone()),
            filename: Set(filename),
            content_type: Set(content_type.map(str::to_string)),
            byte_size: Set(byte_size),
            checksum: Set(checksum),
            created_at: Set(now),
            updated_at: Set(now),
        };

        match active.insert(db).await {
            Ok(model) => Ok(model),
            Err(db_err) => {
                let _ = self.delete(&key).await;
                Err(StorageError::DatabaseError(db_err))
            }
        }
    }

    /// Link a stored file to any record via a polymorphic `file_attachments` row.
    pub async fn attach(
        &self,
        db: &DatabaseConnection,
        file_id: Uuid,
        name: impl Into<String>,
        record_type: impl Into<String>,
        record_id: Uuid,
    ) -> Result<file_attachment::Model, StorageError> {
        let now = Utc::now().naive_utc();
        let active = file_attachment::ActiveModel {
            id: Set(Uuid::new_v4()),
            name: Set(name.into()),
            record_type: Set(record_type.into()),
            record_id: Set(record_id),
            file_id: Set(file_id),
            created_at: Set(now),
            updated_at: Set(now),
        };
        Ok(active.insert(db).await?)
    }

    /// Remove attachments matching the given name/record and delete any files that become orphaned.
    pub async fn detach(
        &self,
        db: &DatabaseConnection,
        name: impl Into<String>,
        record_type: impl Into<String>,
        record_id: Uuid,
    ) -> Result<(), StorageError> {
        let name = name.into();
        let record_type = record_type.into();

        let attachments = file_attachment::Entity::find()
            .filter(file_attachment::Column::Name.eq(&name))
            .filter(file_attachment::Column::RecordType.eq(&record_type))
            .filter(file_attachment::Column::RecordId.eq(record_id))
            .all(db)
            .await?;

        for attachment in attachments {
            let file_id = attachment.file_id;

            file_attachment::Entity::delete_by_id(attachment.id)
                .exec(db)
                .await?;

            let remaining = file_attachment::Entity::find()
                .filter(file_attachment::Column::FileId.eq(file_id))
                .count(db)
                .await?;

            if remaining == 0 {
                if let Some(file) = file::Entity::find_by_id(file_id).one(db).await? {
                    let _ = self.delete(&file.key).await;
                    file::Entity::delete_by_id(file_id).exec(db).await?;
                }
            }
        }

        Ok(())
    }

    pub async fn download(&self, key: &str) -> Result<Bytes, StorageError> {
        match self {
            Self::Local(s) => s.download(key).await,
            Self::S3(s) => s.download(key).await,
            Self::Mock(m) => m
                .lock()
                .unwrap()
                .get(key)
                .cloned()
                .ok_or_else(|| StorageError::NotFound(key.to_string())),
        }
    }

    pub async fn delete(&self, key: &str) -> Result<(), StorageError> {
        match self {
            Self::Local(s) => s.delete(key).await,
            Self::S3(s) => s.delete(key).await,
            Self::Mock(m) => {
                m.lock().unwrap().remove(key);
                Ok(())
            }
        }
    }

    pub async fn url(&self, key: &str, expires_in: Duration) -> Result<String, StorageError> {
        match self {
            Self::Local(s) => s.url(key, expires_in).await,
            Self::S3(s) => s.url(key, expires_in).await,
            Self::Mock(_) => Ok(format!("/storage/{}", key)),
        }
    }

    async fn upload_raw(
        &self,
        key: &str,
        data: Bytes,
        content_type: Option<&str>,
    ) -> Result<(), StorageError> {
        match self {
            Self::Local(s) => s.upload(key, data, content_type).await,
            Self::S3(s) => s.upload(key, data, content_type).await,
            Self::Mock(m) => {
                m.lock().unwrap().insert(key.to_string(), data);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use bytes::Bytes;
    use sea_orm::EntityTrait;
    use uuid::Uuid;

    use super::{file, file_attachment, FileStorage};
    use crate::{app::App, database::migrations::Migrator, tests::setup_test::setup_test};

    fn no_router(app: App) -> Router {
        Router::new()
    }

    fn no_fixtures(
        db: &sea_orm::DatabaseConnection,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let _ = db;
        })
    }

    #[tokio::test]
    async fn test_store_creates_file_record() {
        let t = setup_test::<Migrator>(no_router, no_fixtures).await;
        let storage = FileStorage::mock();

        let file = storage
            .store(&t.db, "hello.txt", Some("text/plain"), Bytes::from("hello world"))
            .await
            .unwrap();

        assert_eq!(file.filename, "hello.txt");
        assert_eq!(file.content_type.as_deref(), Some("text/plain"));
        assert_eq!(file.byte_size, 11);
        assert!(!file.checksum.is_empty());

        let stored = file::Entity::find_by_id(file.id).one(&t.db).await.unwrap();
        assert!(stored.is_some());
    }

    #[tokio::test]
    async fn test_attach_creates_attachment_record() {
        let t = setup_test::<Migrator>(no_router, no_fixtures).await;
        let storage = FileStorage::mock();

        let file = storage
            .store(&t.db, "photo.jpg", Some("image/jpeg"), Bytes::from("fake image"))
            .await
            .unwrap();

        let record_id = Uuid::new_v4();
        let attachment = storage
            .attach(&t.db, file.id, "avatar", "user", record_id)
            .await
            .unwrap();

        assert_eq!(attachment.name, "avatar");
        assert_eq!(attachment.record_type, "user");
        assert_eq!(attachment.record_id, record_id);
        assert_eq!(attachment.file_id, file.id);
    }

    #[tokio::test]
    async fn test_detach_removes_attachment_and_orphaned_file() {
        let t = setup_test::<Migrator>(no_router, no_fixtures).await;
        let storage = FileStorage::mock();

        let file = storage
            .store(&t.db, "doc.pdf", Some("application/pdf"), Bytes::from("pdf content"))
            .await
            .unwrap();
        let file_id = file.id;
        let record_id = Uuid::new_v4();

        storage
            .attach(&t.db, file_id, "document", "post", record_id)
            .await
            .unwrap();
        storage
            .detach(&t.db, "document", "post", record_id)
            .await
            .unwrap();

        let remaining_attachment = file_attachment::Entity::find()
            .one(&t.db)
            .await
            .unwrap();
        assert!(remaining_attachment.is_none());

        let remaining_file = file::Entity::find_by_id(file_id).one(&t.db).await.unwrap();
        assert!(remaining_file.is_none(), "orphaned file should be deleted");
    }

    #[tokio::test]
    async fn test_detach_keeps_file_when_other_attachments_exist() {
        let t = setup_test::<Migrator>(no_router, no_fixtures).await;
        let storage = FileStorage::mock();

        let file = storage
            .store(&t.db, "shared.png", Some("image/png"), Bytes::from("image data"))
            .await
            .unwrap();
        let file_id = file.id;

        let record_a = Uuid::new_v4();
        let record_b = Uuid::new_v4();
        storage
            .attach(&t.db, file_id, "thumbnail", "post", record_a)
            .await
            .unwrap();
        storage
            .attach(&t.db, file_id, "thumbnail", "post", record_b)
            .await
            .unwrap();

        storage
            .detach(&t.db, "thumbnail", "post", record_a)
            .await
            .unwrap();

        let remaining_file = file::Entity::find_by_id(file_id).one(&t.db).await.unwrap();
        assert!(
            remaining_file.is_some(),
            "file should be kept while other attachments exist"
        );
    }

    #[tokio::test]
    async fn test_mock_download_roundtrip() {
        let storage = FileStorage::mock();
        let data = Bytes::from("roundtrip content");
        storage
            .upload_raw("test/key", data.clone(), None)
            .await
            .unwrap();
        let downloaded = storage.download("test/key").await.unwrap();
        assert_eq!(downloaded, data);
    }

    #[tokio::test]
    async fn test_mock_download_missing_key_returns_not_found() {
        let storage = FileStorage::mock();
        let err = storage.download("nonexistent/key").await.unwrap_err();
        assert!(matches!(err, super::StorageError::NotFound(_)));
    }
}
