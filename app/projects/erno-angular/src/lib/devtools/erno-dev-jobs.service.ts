import { Inject, Injectable } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { Observable } from 'rxjs';
import { ERNO_CONFIG, ErnoConfig } from '../erno.config';

export interface DevJob {
  id: string;
  type: string;
  arguments: unknown;
  status: 'pending' | 'pending_retry' | 'running' | 'completed' | 'failed';
  retry_count: number;
  next_execution_at: string | null;
  created_at: string;
  updated_at: string;
}

@Injectable()
export class ErnoDevJobsService {
  constructor(
    private http: HttpClient,
    @Inject(ERNO_CONFIG) private config: ErnoConfig,
  ) {}

  list(): Observable<DevJob[]> {
    return this.http.get<DevJob[]>(`${this.config.baseUrl}/dev/jobs`);
  }

  clear(): Observable<void> {
    return this.http.delete<void>(`${this.config.baseUrl}/dev/jobs`);
  }
}
