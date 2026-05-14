import { inject, Injectable } from '@angular/core';
import { HttpClient, HttpEvent, HttpHeaders, HttpRequest } from '@angular/common/http';
import { Observable } from 'rxjs';
import { ERNO_CONFIG } from '../erno.config';

export interface UploadUrlResponse {
  upload_url: string;
  file_path: string;
}

@Injectable()
export class ErnoStorageService {
  private config = inject(ERNO_CONFIG);
  private http = inject(HttpClient);

  getUploadUrl(filename: string, contentType: string): Observable<UploadUrlResponse> {
    return this.http.post<UploadUrlResponse>(`${this.config.baseUrl}/storage/upload-url`, { filename, content_type: contentType });
  }

  upload(file: File, uploadUrl: string): Observable<HttpEvent<unknown>> {
    const req = new HttpRequest('PUT', uploadUrl, file, {
      reportProgress: true,
      headers: new HttpHeaders({ 'Content-Type': file.type }),
    });
    return this.http.request(req);
  }

  getDownloadUrl(filePath: string): Observable<{ url: string }> {
    return this.http.get<{ url: string }>(`${this.config.baseUrl}/storage/download-url`, { params: { path: filePath } });
  }

  delete(filePath: string): Observable<void> {
    return this.http.delete<void>(`${this.config.baseUrl}/storage/files`, { params: { path: filePath } });
  }
}
