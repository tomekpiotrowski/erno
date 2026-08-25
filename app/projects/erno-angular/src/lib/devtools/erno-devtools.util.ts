import { DevJob, DevJobExecution, DevJobStatus } from './erno-dev-jobs.service';

export type JobKindFilter = 'all' | 'attention' | 'failed';
export type Tone = 'ok' | 'warn' | 'err' | 'muted';

export interface JobGroup {
  type: string;
  jobs: DevJob[];
  status: DevJobStatus;
  runCount: number;
  avgMs: number | null;
  error: string | null;
  failedJobId: string | null;
}

export interface JobRunRow {
  id: string;
  ms: number | null;
  state: string;
}

const STATUS_RANK: Record<DevJobStatus, number> = {
  failed: 0,
  pending_retry: 1,
  running: 2,
  pending: 3,
  completed: 4,
};

export function groupJobs(jobs: DevJob[]): JobGroup[] {
  const map = new Map<string, DevJob[]>();
  for (const job of jobs) {
    const list = map.get(job.type) ?? [];
    list.push(job);
    map.set(job.type, list);
  }
  return [...map.entries()].map(([type, items]) => toGroup(type, items));
}

function rank(status: DevJobStatus): number {
  return STATUS_RANK[status] ?? 5;
}

function toGroup(type: string, items: DevJob[]): JobGroup {
  const status = items.reduce(
    (worst, job) => (rank(job.status) < rank(worst) ? job.status : worst),
    items[0].status,
  );
  const times = items
    .map(job => job.executions?.[0]?.execution_time_ms)
    .filter((n): n is number => n != null);
  const avgMs = times.length ? Math.round(times.reduce((a, b) => a + b, 0) / times.length) : null;
  const failed = items.find(job => job.status === 'failed');
  const error =
    failed?.executions?.find(e => e.failure_reason)?.failure_reason ??
    failed?.executions?.[0]?.failure_reason ??
    null;
  return {
    type,
    jobs: items,
    status,
    runCount: items.length,
    avgMs,
    error,
    failedJobId: failed?.id ?? null,
  };
}

export function filterJobGroups(
  groups: JobGroup[],
  query: string,
  filter: JobKindFilter,
): JobGroup[] {
  const q = query.trim().toLowerCase();
  return groups.filter(group => {
    if (q && !group.type.toLowerCase().includes(q)) return false;
    if (filter === 'failed') return group.status === 'failed';
    if (filter === 'attention') {
      return (
        group.status === 'failed' ||
        group.status === 'running' ||
        group.status === 'pending_retry'
      );
    }
    return true;
  });
}

export function groupRuns(group: JobGroup): JobRunRow[] {
  if (group.jobs.length === 1 && (group.jobs[0].executions?.length ?? 0) > 0) {
    return group.jobs[0].executions.slice(0, 8).map(executionToRun);
  }
  return group.jobs.slice(0, 8).map(jobToRun);
}

function executionToRun(e: DevJobExecution): JobRunRow {
  return { id: shortId(e.id), ms: e.execution_time_ms, state: e.result };
}

function jobToRun(job: DevJob): JobRunRow {
  return {
    id: shortId(job.id),
    ms: job.executions?.[0]?.execution_time_ms ?? null,
    state: job.status,
  };
}

export function shortId(id: string): string {
  return id.replace(/-/g, '').slice(0, 8);
}

export function statusLabel(status: string): string {
  switch (status) {
    case 'failed':
    case 'timed_out':
      return 'Failed';
    case 'running':
      return 'Running';
    case 'pending_retry':
      return 'Retrying';
    case 'pending':
      return 'Pending';
    case 'completed':
      return 'Completed';
    default:
      return status;
  }
}

export function statusTone(status: string): Tone {
  const s = status.toLowerCase();
  if (s === 'failed' || s === 'timed_out') return 'err';
  if (s === 'running' || s === 'pending_retry' || s === 'pending') return 'warn';
  return 'ok';
}

export function formatClock(value: string | number | Date | null | undefined): string {
  if (value == null) return '';
  const d = new Date(value);
  if (Number.isNaN(d.getTime())) return '';
  return [d.getHours(), d.getMinutes(), d.getSeconds()]
    .map(n => String(n).padStart(2, '0'))
    .join(':');
}

export function formatUptime(fromMs: number, nowMs: number): string {
  const min = Math.max(0, Math.floor((nowMs - fromMs) / 60000));
  return `${Math.floor(min / 60)}h${String(min % 60).padStart(2, '0')}m`;
}

export function formatMs(ms: number | null): string {
  if (ms == null) return '—';
  return `${ms}ms`;
}

export function apiHost(baseUrl: string): string {
  try {
    const u = new URL(baseUrl);
    return u.port ? `${u.hostname}:${u.port}` : u.hostname;
  } catch {
    return baseUrl.replace(/^https?:\/\//, '');
  }
}

export function syncLabel(status: string): string {
  switch (status) {
    case 'syncing':
      return 'syncing';
    case 'synced':
      return 'in step';
    case 'offline':
      return 'offline';
    case 'error':
      return 'error';
    default:
      return 'idle';
  }
}

export function syncTone(status: string): Tone {
  if (status === 'error' || status === 'offline') return 'err';
  if (status === 'syncing') return 'warn';
  return 'ok';
}
