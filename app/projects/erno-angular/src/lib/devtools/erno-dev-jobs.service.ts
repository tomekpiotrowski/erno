import { Inject, Injectable } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { Observable } from 'rxjs';
import { ERNO_CONFIG, ErnoConfig } from '../erno.config';

export type DevJobStatus = 'pending' | 'pending_retry' | 'running' | 'completed' | 'failed';

export interface DevJobExecution {
  id: string;
  result: 'completed' | 'failed' | 'timed_out' | string;
  execution_time_ms: number;
  failure_reason: string | null;
  started_at: string;
  finished_at: string;
}

export interface DevJob {
  id: string;
  type: string;
  arguments: unknown;
  status: DevJobStatus;
  retry_count: number;
  next_execution_at: string | null;
  created_at: string;
  updated_at: string;
  executions: DevJobExecution[];
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

  retry(id: string): Observable<void> {
    return this.http.post<void>(`${this.config.baseUrl}/dev/jobs/${id}/retry`, {});
  }

  clear(): Observable<void> {
    return this.http.delete<void>(`${this.config.baseUrl}/dev/jobs`);
  }
}
