---
title: File Storage
description: Local and S3 file storage with polymorphic attachments
sidebar:
  order: 11
---

> **Source**: `api/src/storage/`

`FileStorage` supports three backends: local disk, S3-compatible object stores (AWS S3, Digital Ocean Spaces, MinIO), and an in-memory mock for tests. The active instance is available on `app.storage`.

## Storing a file

`store` uploads bytes to the configured backend, computes a SHA-256 checksum, and inserts a row in the `files` table. If the database insert fails the uploaded bytes are deleted automatically to avoid orphans.

```rust
use bytes::Bytes;

let data = Bytes::from(file_bytes);
let file = app.storage
    .store(&app.db, "avatar.jpg", Some("image/jpeg"), data)
    .await?;

// file.id  — UUID primary key, use this for attachments
// file.key — storage key (opaque path), use this for download/url
```

## Polymorphic attachments

Attach a stored file to any record type using a name + record type + record ID triple:

```rust
// attach
let attachment = app.storage
    .attach(&app.db, file.id, "avatar", "user", user.id)
    .await?;

// detach (removes the attachment; deletes the file if it becomes orphaned)
app.storage
    .detach(&app.db, "avatar", "user", user.id)
    .await?;
```

A file is deleted only when its last attachment is removed.

## Downloading and URLs

```rust
// download raw bytes
let bytes = app.storage.download(&file.key).await?;

// get a URL (presigned for S3, /storage/{key} for local)
use std::time::Duration;
let url = app.storage.url(&file.key, Duration::from_secs(3600)).await?;

// delete
app.storage.delete(&file.key).await?;
```

For the S3 backend, `url` returns a presigned URL valid for `expires_in`. If `storage.s3.cdn_endpoint` is set, the CDN prefix is used instead of a presigned URL. For the local backend, `url` returns `/storage/{key}` — serve this path from your app.

## Configuration

### Local

```toml
[storage]
backend = "local"
local_path = "./storage"   # default
```

### S3 / S3-compatible

```toml
[storage]
backend = "s3"

[storage.s3]
bucket = "my-bucket"
region = "us-east-1"
access_key_id = "AKIA..."
secret_access_key = "..."
endpoint = "https://nyc3.digitaloceanspaces.com"  # optional; omit for AWS
cdn_endpoint = "https://cdn.example.com"           # optional; used by url()
```

## Testing

Use `FileStorage::mock()` in tests — in-memory, no real backend needed:

```rust
let storage = FileStorage::mock();

let file = storage
    .store(&db, "test.txt", Some("text/plain"), Bytes::from("hello"))
    .await?;

let downloaded = storage.download(&file.key).await?;
```

The `setup_test` helper (see [Database](../database)) automatically provides a mock storage instance via `app.storage`.
