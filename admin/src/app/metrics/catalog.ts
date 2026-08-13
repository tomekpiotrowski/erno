export const WINDOWS = [
  { id: '1h', seconds: 3600, step: '15s' },
  { id: '6h', seconds: 21600, step: '60s' },
  { id: '24h', seconds: 86400, step: '5m' },
  { id: '7d', seconds: 604800, step: '30m' },
  { id: '90d', seconds: 7776000, step: '6h' },
] as const;

export const PERFORMANCE = {
  requestRate: 'sum by (method, path) (rate(http_requests_total[5m]))',
  errorRate: 'sum by (path) (rate(http_requests_total{status=~"5.."}[5m]))',
  latencyP50:
    'histogram_quantile(0.50, sum by (le, path) (rate(http_request_duration_seconds_bucket[5m])))',
  latencyP95:
    'histogram_quantile(0.95, sum by (le, path) (rate(http_request_duration_seconds_bucket[5m])))',
  latencyP99:
    'histogram_quantile(0.99, sum by (le, path) (rate(http_request_duration_seconds_bucket[5m])))',
  inFlight: 'http_requests_in_flight',
  poolTotal: 'db_pool_connections_total',
  poolIdle: 'db_pool_connections_idle',
  jobQueue: 'jobs_pending_count',
};

export const BUSINESS = {
  users: 'erno_users_total',
  paid: 'erno_users_paid',
  trial: 'erno_users_trial',
  gift: 'erno_users_gift',
  none: 'erno_users_none',
  active1d: 'erno_users_active_1d',
  active7d: 'erno_users_active_7d',
  active30d: 'erno_users_active_30d',
  registered: 'increase(erno_users_registered_total[24h])',
  deleted: 'increase(erno_users_deleted_total[24h])',
  tableCount: 'db_table_count',
  cubeastSolves: 'increase(cubeast_solves_created_total[24h])',
  cubeastSessions: 'increase(cubeast_sessions_created_total[24h])',
};
