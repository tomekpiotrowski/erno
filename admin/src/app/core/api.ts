import { HttpClient, HttpParams } from '@angular/common/http';
import { inject, Injectable } from '@angular/core';

@Injectable({ providedIn: 'root' })
export class AdminApi {
  private readonly http = inject(HttpClient);

  dashboard() {
    return this.http.get<Dashboard>('/admin/api/dashboard');
  }

  users(q = '', page = 1, perPage = 50) {
    let params = new HttpParams().set('page', page).set('per_page', perPage);
    if (q) params = params.set('q', q);
    return this.http.get<UserList>('/admin/api/users', { params });
  }

  user(id: string) {
    return this.http.get<UserDetail>(`/admin/api/users/${id}`);
  }

  activate(id: string) {
    return this.http.post<UserDetail>(`/admin/api/users/${id}/activate`, {});
  }

  gift(id: string, plan: string, durationDays: number) {
    return this.http.post<UserDetail>(`/admin/api/users/${id}/gift`, {
      plan,
      duration_days: durationDays,
    });
  }

  deleteUser(id: string) {
    return this.http.delete(`/admin/api/users/${id}`);
  }

  jobs(status = '', type = '') {
    let params = new HttpParams();
    if (status) params = params.set('status', status);
    if (type) params = params.set('type', type);
    return this.http.get<JobsResponse>('/admin/api/jobs', { params });
  }

  job(id: string) {
    return this.http.get<JobDetail>(`/admin/api/jobs/${id}`);
  }

  retry(id: string) {
    return this.http.post(`/admin/api/jobs/${id}/retry`, {});
  }

  emails(to = '') {
    let params = new HttpParams();
    if (to) params = params.set('to', to);
    return this.http.get<EmailList>('/admin/api/emails', { params });
  }

  email(id: string) {
    return this.http.get<EmailMessage>(`/admin/api/emails/${id}`);
  }

  tables() {
    return this.http.get<TablesResponse>('/admin/api/tables');
  }

  events(name = '', days = 7) {
    let params = new HttpParams().set('days', days);
    if (name) params = params.set('name', name);
    return this.http.get<EventsResponse>('/admin/api/events', { params });
  }

  plans() {
    return this.http.get<{ plans: string[] }>('/admin/api/plans');
  }
}

export interface Dashboard {
  total_users: number;
  stripe_active: number;
  gift_active: number;
  trial_active: number;
  no_sub: number;
  pending_jobs: number;
  running_jobs: number;
  failed_jobs: number;
  completed_jobs_1h: number;
  failed_executions_1h: number;
  timed_out_1h: number;
  avg_execution_ms: number;
  email_stats: { name: string; total: number; completed: number; failed: number }[];
  refreshed_at: string;
}

export interface UserSummary {
  id: string;
  email: string;
  email_verified_at: string | null;
  last_active_at: string | null;
  subscription_type: string | null;
  subscription_plan: string | null;
  created_at: string;
}

export interface UserList {
  users: UserSummary[];
  page: number;
  per_page: number;
  total: number;
}

export interface SubscriptionInfo {
  sub_type: string;
  plan: string;
  status: string;
  expiry: string;
  stripe_customer_id: string | null;
  stripe_sub_id: string | null;
  cancel_at_period_end: boolean | null;
}

export interface UserDetail {
  user: UserSummary;
  subscription: SubscriptionInfo | null;
  oauth_providers: string[];
  subscription_history: SubscriptionInfo[];
}

export interface JobsResponse {
  stats: { job_type: string; pending: number; running: number; failed: number; completed: number }[];
  jobs: {
    id: string;
    job_type: string;
    status: string;
    retry_count: number;
    created_at: string;
    next_execution_at: string | null;
  }[];
}

export interface JobDetail {
  job: JobsResponse['jobs'][0];
  arguments: unknown;
  executions: {
    id: string;
    result: string;
    started_at: string;
    finished_at: string;
    execution_time_ms: number;
    failure_reason: string | null;
  }[];
}

export interface EmailMessage {
  id: string;
  to: string;
  from: string;
  subject: string;
  template: string | null;
  status: string;
  error: string | null;
  sent_at: string | null;
  created_at: string;
}

export interface EmailList {
  emails: EmailMessage[];
  page: number;
  per_page: number;
  total: number;
}

export interface TablesResponse {
  tables: {
    table: string;
    approx_rows: number;
    n_dead_tup: number;
    last_analyze: string | null;
    approx: boolean;
  }[];
}

export interface EventsResponse {
  events: {
    id: string;
    name: string;
    user_id: string | null;
    payload: unknown;
    created_at: string;
  }[];
}
