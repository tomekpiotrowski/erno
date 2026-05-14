use std::time::Duration;

use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_s3::{
    config::{Credentials, Region},
    presigning::PresigningConfig,
    Client,
};
use bytes::Bytes;

use super::{error::StorageError, service::StorageService};
use crate::config::S3Config;

#[derive(Clone)]
pub struct S3Storage {
    client: Client,
    bucket: String,
    cdn_endpoint: Option<String>,
}

impl S3Storage {
    pub fn new(config: &S3Config) -> Self {
        let credentials = Credentials::new(
            &config.access_key_id,
            &config.secret_access_key,
            None,
            None,
            "erno-storage",
        );

        let mut builder = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new(config.region.clone()))
            .credentials_provider(credentials);

        if let Some(endpoint) = &config.endpoint {
            builder = builder.endpoint_url(endpoint);
        }

        Self {
            client: Client::from_conf(builder.build()),
            bucket: config.bucket.clone(),
            cdn_endpoint: config.cdn_endpoint.clone(),
        }
    }
}

#[async_trait]
impl StorageService for S3Storage {
    async fn upload(&self, key: &str, data: Bytes, content_type: Option<&str>) -> Result<(), StorageError> {
        let mut request = self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(data.into());

        if let Some(ct) = content_type {
            request = request.content_type(ct);
        }

        request.send().await
            .map_err(|e| StorageError::UploadFailed(e.to_string()))?;
        Ok(())
    }

    async fn download(&self, key: &str) -> Result<Bytes, StorageError> {
        let response = self.client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("NoSuchKey") || msg.contains("404") {
                    StorageError::NotFound(key.to_string())
                } else {
                    StorageError::DownloadFailed(msg)
                }
            })?;

        let data = response.body.collect().await
            .map_err(|e| StorageError::DownloadFailed(e.to_string()))?;
        Ok(data.into_bytes())
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| StorageError::DeleteFailed(e.to_string()))?;
        Ok(())
    }

    async fn url(&self, key: &str, expires_in: Duration) -> Result<String, StorageError> {
        if let Some(cdn) = &self.cdn_endpoint {
            return Ok(format!("{}/{}", cdn.trim_end_matches('/'), key));
        }

        let presigning_config = PresigningConfig::expires_in(expires_in)
            .map_err(|e| StorageError::UrlGenerationFailed(e.to_string()))?;

        let presigned = self.client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(presigning_config)
            .await
            .map_err(|e| StorageError::UrlGenerationFailed(e.to_string()))?;

        Ok(presigned.uri().to_string())
    }
}
