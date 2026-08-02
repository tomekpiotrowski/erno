---
title: File storage
description: ErnoStorageService — upload and download via backend storage
sidebar:
  order: 5
---

`ErnoStorageService` obtains signed (or local) upload/download URLs from the Erno API and performs the binary transfer. The server owns the `files` table and backend choice (disk, S3, mock); the client only moves bytes.

> **Backend counterpart**: [File storage (API)](/api/storage/)

## Upload flow

```typescript
import { ErnoStorageService } from 'erno-angular';
import { HttpEventType } from '@angular/common/http';

constructor(private storage: ErnoStorageService) {}

async uploadAvatar(file: File) {
  const { upload_url, file_path } = await firstValueFrom(
    this.storage.getUploadUrl(file.name, file.type),
  );

  this.storage.upload(file, upload_url).subscribe(event => {
    if (event.type === HttpEventType.UploadProgress && event.total) {
      const pct = Math.round((100 * event.loaded) / event.total);
      console.log('progress', pct);
    }
    if (event.type === HttpEventType.Response) {
      // Persist file_path on your domain model (e.g. user.avatar_path)
      console.log('stored as', file_path);
    }
  });
}
```

1. `getUploadUrl` — `POST /api/storage/upload-url` with `{ filename, content_type }` → `{ upload_url, file_path }`.
2. `upload` — `PUT` the file bytes to `upload_url` with progress events.
3. Store `file_path` on your entity (or attach server-side via the API storage attachment APIs).

## Download and delete

```typescript
// Temporary download URL
this.storage.getDownloadUrl(filePath).subscribe(({ url }) => {
  window.open(url, '_blank');
});

// Remove object + metadata on the server
this.storage.delete(filePath).subscribe();
```

| Member | Description |
|--------|-------------|
| `getUploadUrl(filename, contentType)` | Allocates a storage key and returns a write URL |
| `upload(file, uploadUrl)` | `HttpRequest` PUT with `reportProgress: true` |
| `getDownloadUrl(filePath)` | Returns a read URL for the given path |
| `delete(filePath)` | Deletes the file via the API |

## Notes

- All methods use the configured `baseUrl` and go through the auth interceptor (except the PUT to an external signed URL host, which is a full URL and is not token-injected by the interceptor’s `baseUrl` prefix check).
- Content type on upload is taken from `file.type`; pass an accurate type when requesting the upload URL so the backend can store it.
