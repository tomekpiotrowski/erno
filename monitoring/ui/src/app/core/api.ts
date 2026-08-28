import { Injectable, inject } from '@angular/core';
import { HttpClient, HttpParams } from '@angular/common/http';
import { ProjectContext } from './project';

// Docs: docs/src/content/docs/monitoring/error-reporting.md
//
// Field names are the collector's snake_case verbatim, matching the convention
// in the admin console's client — there is no conversion layer to keep in sync.

export interface IssueSummary {
  id: string;
  fingerprint: string;
  source: string;
  error_type: string;
  title: string;
  culprit: string | null;
  level: string;
  status: string;
  /**
   * Lifetime occurrences. A floor rather than an exact count whenever reports
   * were shed under load, which is why the detail screen shows it alongside
   * `stored_events` instead of implying they are the same number.
   */
  times_seen: number;
  first_seen: string;
  last_seen: string;
  first_release: string | null;
  last_release: string | null;
  environment: string | null;
  /** Which application this issue belongs to. */
  project_slug: string;
  project_name: string;
}

export interface IssueList {
  issues: IssueSummary[];
  page: number;
  per_page: number;
  total: number;
}

export interface StackFrame {
  function?: string | null;
  file?: string | null;
  line?: number | null;
  column?: number | null;
  in_app: boolean;
}

export interface ErrorEvent {
  id: string;
  issue_id: string;
  source: string;
  level: string;
  error_type: string;
  message: string;
  stack: string | null;
  frames: StackFrame[] | null;
  context: Record<string, unknown>;
  release: string | null;
  environment: string | null;
  user_id: string | null;
  user_email: string | null;
  created_at: string;
}

export interface IssueDetail {
  issue: IssueSummary;
  stored_events: number;
  latest_event: ErrorEvent | null;
  events: ErrorEvent[];
}

export interface IssueCounts {
  unresolved: number;
  resolved: number;
  ignored: number;
}

export interface SeriesPoint {
  t: string;
  count: number;
}

export interface Series {
  points: SeriesPoint[];
  bucket: string;
}

/** Shape the shared sparkline component expects. */
export interface PromPoint {
  t: number;
  v: number;
}

/** Map a collector series onto the sparkline's point shape. */
export function toPromPoints(series: Series | null): PromPoint[] {
  if (!series) {
    return [];
  }
  return series.points.map((p) => ({ t: Date.parse(`${p.t}Z`), v: p.count }));
}

export interface Release {
  id: string;
  version: string;
  environment: string;
  commit_sha: string | null;
  source: string | null;
  deployed_at: string;
  /** Issues whose first sighting carried this version. */
  new_issues: number;
}

export interface ReleaseList {
  releases: Release[];
}

export type HealthState = 'ok' | 'degraded' | 'down';

export interface SubsystemStatus {
  name: string;
  state: HealthState;
  detail: string;
}

export interface InstanceHealth {
  instance: string;
  environment: string;
  release: string | null;
  reported_at: string;
  age_seconds: number;
  stale: boolean;
  state: HealthState;
  subsystems: SubsystemStatus[];
}

export interface HealthResponse {
  instances: InstanceHealth[];
  state: HealthState;
}

export interface UptimeCheck {
  id: string;
  name: string;
  url: string;
  method: string;
  expected_status: number;
  interval_seconds: number;
  enabled: boolean;
  state: 'unknown' | 'up' | 'down';
  consecutive_failures: number;
  state_changed_at: string | null;
  last_checked_at: string | null;
  /** Null until the check has actually run — an empty check is unknown, not broken. */
  uptime_ratio: number | null;
  p50_ms: number | null;
  p95_ms: number | null;
}

export interface UptimeList {
  checks: UptimeCheck[];
  window_hours: number;
}

export interface StatusComponent {
  id: string;
  name: string;
  description: string | null;
  position: number;
  auto_from_check_id: string | null;
  manual_state: string;
}

export interface PublicComponent {
  id: string;
  name: string;
  state: string;
  uptime_ratio: number | null;
}

export interface PublicIncident {
  id: string;
  title: string;
  status: string;
  impact: string;
  started_at: string;
  resolved_at: string | null;
  updates: { status: string; body: string; created_at: string }[];
}

export interface StatusSnapshot {
  name: string;
  state: string;
  generated_at: string;
  refresh_seconds: number;
  components: PublicComponent[];
  active_incidents: PublicIncident[];
  recent_incidents: PublicIncident[];
}

export interface AlertRule {
  id: string;
  name: string;
  enabled: boolean;
  source: string;
  selector: string;
  comparator: string;
  threshold: number;
  window_seconds: number;
  for_seconds: number;
  repeat_seconds: number;
  severity: string;
  notify_email: string | null;
  notify_webhook: string | null;
  silence_until: string | null;
  state: 'ok' | 'pending' | 'firing';
  state_since: string | null;
  last_evaluated_at: string | null;
  last_value: string | null;
}

export type IssueStatus = 'unresolved' | 'resolved' | 'ignored' | 'all';

@Injectable({ providedIn: 'root' })
export class CollectorApi {
  private readonly http = inject(HttpClient);
  private readonly project = inject(ProjectContext);

  /**
   * Base path of the project the console is looking at.
   *
   * Every operator route below the project list is nested under a slug, so a
   * request cannot be built before {@link ProjectContext.load} has settled on
   * one.
   */
  private get base(): string {
    return this.project.base;
  }

  issues(
    status: IssueStatus = 'unresolved',
    source = 'all',
    q = '',
    hours = 168,
    page = 1,
    perPage = 50,
    release = '',
  ) {
    let params = new HttpParams()
      .set('status', status)
      .set('hours', hours)
      .set('page', page)
      .set('per_page', perPage);
    if (source && source !== 'all') {
      params = params.set('source', source);
    }
    if (q) {
      params = params.set('q', q);
    }
    if (release) {
      params = params.set('release', release);
    }
    return this.http.get<IssueList>(`${this.base}/issues`, { params });
  }

  counts(hours = 168) {
    return this.http.get<IssueCounts>(`${this.base}/issues/counts`, {
      params: new HttpParams().set('hours', hours),
    });
  }

  issue(id: string) {
    return this.http.get<IssueDetail>(`${this.base}/issues/${id}`);
  }

  issueSeries(id: string, hours = 24) {
    return this.http.get<Series>(`${this.base}/issues/${id}/series`, {
      params: new HttpParams().set('hours', hours),
    });
  }

  series(hours = 24, source = 'all') {
    let params = new HttpParams().set('hours', hours);
    if (source && source !== 'all') {
      params = params.set('source', source);
    }
    return this.http.get<Series>(`${this.base}/series`, { params });
  }

  resolve(id: string) {
    return this.http.post<IssueSummary>(`${this.base}/issues/${id}/resolve`, {});
  }

  ignore(id: string) {
    return this.http.post<IssueSummary>(`${this.base}/issues/${id}/ignore`, {});
  }

  unresolve(id: string) {
    return this.http.post<IssueSummary>(`${this.base}/issues/${id}/unresolve`, {});
  }

  remove(id: string) {
    return this.http.delete<void>(`${this.base}/issues/${id}`);
  }

  releases(environment = 'all', limit = 20) {
    let params = new HttpParams().set('limit', limit);
    if (environment && environment !== 'all') {
      params = params.set('environment', environment);
    }
    return this.http.get<ReleaseList>(`${this.base}/releases`, { params });
  }

  health() {
    return this.http.get<HealthResponse>(`${this.base}/health`);
  }

  uptime(hours = 24) {
    return this.http.get<UptimeList>(`${this.base}/uptime`, {
      params: new HttpParams().set('hours', hours),
    });
  }

  createCheck(body: {
    name: string;
    url: string;
    interval_seconds?: number;
    expected_status?: number;
  }) {
    return this.http.post<{ id: string }>(`${this.base}/uptime`, body);
  }

  deleteCheck(id: string) {
    return this.http.delete<void>(`${this.base}/uptime/${id}`);
  }

  setCheckEnabled(id: string, enabled: boolean) {
    return this.http.post<{ id: string; enabled: boolean }>(
      `${this.base}/uptime/${id}/${enabled ? 'enable' : 'disable'}`,
      {},
    );
  }

  statusSnapshot() {
    return this.http.get<StatusSnapshot>(`${this.base}/status.json`);
  }

  statusComponents() {
    return this.http.get<{ components: StatusComponent[] }>(`${this.base}/status/components`);
  }

  createStatusComponent(body: {
    name: string;
    description?: string;
    auto_from_check_id?: string | null;
  }) {
    return this.http.post<{ id: string }>(`${this.base}/status/components`, body);
  }

  deleteStatusComponent(id: string) {
    return this.http.delete<void>(`${this.base}/status/components/${id}`);
  }

  setStatusComponentState(id: string, state: string) {
    return this.http.post(`${this.base}/status/components/${id}/state`, { state });
  }

  openIncident(body: { title: string; impact: string; body: string }) {
    return this.http.post<{ id: string }>(`${this.base}/status/incidents`, body);
  }

  addIncidentUpdate(id: string, body: { status: string; body: string }) {
    return this.http.post(`${this.base}/status/incidents/${id}/updates`, body);
  }

  alertRules() {
    return this.http.get<{ rules: AlertRule[] }>(`${this.base}/alerts`);
  }

  createAlertRule(body: {
    name: string;
    source: string;
    selector?: string;
    comparator?: string;
    threshold: number;
    for_seconds?: number;
    notify_email?: string;
  }) {
    return this.http.post<{ id: string }>(`${this.base}/alerts`, body);
  }

  deleteAlertRule(id: string) {
    return this.http.delete<void>(`${this.base}/alerts/${id}`);
  }

  setAlertRuleEnabled(id: string, enabled: boolean) {
    return this.http.post(`${this.base}/alerts/${id}/${enabled ? 'enable' : 'disable'}`, {});
  }

  silenceAlertRule(id: string, minutes: number) {
    return this.http.post(`${this.base}/alerts/${id}/silence`, { minutes });
  }

  /** This console reporting its own errors. */
  report(body: unknown) {
    return this.http.post<void>('/api/errors', body);
  }
}
