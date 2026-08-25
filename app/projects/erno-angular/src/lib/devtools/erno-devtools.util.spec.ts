import { DevJob } from './erno-dev-jobs.service';
import {
  apiHost,
  filterJobGroups,
  formatClock,
  formatMs,
  formatUptime,
  groupJobs,
  groupRuns,
  statusLabel,
  statusTone,
  syncLabel,
} from './erno-devtools.util';

function job(partial: Partial<DevJob> & Pick<DevJob, 'id' | 'type'>): DevJob {
  return {
    arguments: {},
    status: 'completed',
    retry_count: 0,
    next_execution_at: null,
    created_at: '2026-08-25T12:00:00',
    updated_at: '2026-08-25T12:00:00',
    executions: [],
    ...partial,
  };
}

describe('erno-devtools grouping', () => {
  it('groups jobs by type and ranks the worst status first', () => {
    const groups = groupJobs([
      job({ id: '1', type: 'send_mail', status: 'completed' }),
      job({
        id: '2',
        type: 'charge',
        status: 'failed',
        executions: [
          {
            id: 'e1',
            result: 'failed',
            execution_time_ms: 240,
            failure_reason: 'PoolTimedOut',
            started_at: '2026-08-25T12:00:00',
            finished_at: '2026-08-25T12:00:00',
          },
        ],
      }),
      job({ id: '3', type: 'send_mail', status: 'running' }),
    ]);

    expect(groups.map(g => g.type)).toEqual(['send_mail', 'charge']);
    expect(groups[0].runCount).toBe(2);
    expect(groups[0].status).toBe('running');
    expect(groups[1].status).toBe('failed');
    expect(groups[1].error).toBe('PoolTimedOut');
    expect(groups[1].avgMs).toBe(240);
  });

  it('filters by query and attention/failed chips', () => {
    const groups = groupJobs([
      job({ id: '1', type: 'send_mail', status: 'completed' }),
      job({ id: '2', type: 'charge', status: 'failed' }),
      job({ id: '3', type: 'rebuild_search', status: 'running' }),
    ]);

    expect(filterJobGroups(groups, 'charge', 'all').map(g => g.type)).toEqual(['charge']);
    expect(filterJobGroups(groups, '', 'failed').map(g => g.type)).toEqual(['charge']);
    expect(filterJobGroups(groups, '', 'attention').map(g => g.type)).toEqual([
      'charge',
      'rebuild_search',
    ]);
  });

  it('expands a single job to its executions, otherwise lists sibling jobs', () => {
    const single = groupJobs([
      job({
        id: 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee',
        type: 'charge',
        status: 'failed',
        executions: [
          {
            id: '11111111-2222-3333-4444-555555555555',
            result: 'failed',
            execution_time_ms: 30,
            failure_reason: 'boom',
            started_at: 't',
            finished_at: 't',
          },
          {
            id: '66666666-7777-8888-9999-000000000000',
            result: 'completed',
            execution_time_ms: 12,
            failure_reason: null,
            started_at: 't',
            finished_at: 't',
          },
        ],
      }),
    ])[0];
    expect(groupRuns(single).map(r => r.state)).toEqual(['failed', 'completed']);
    expect(groupRuns(single)[0].id).toBe('11111111');

    const siblings = groupJobs([
      job({ id: 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee', type: 'tick' }),
      job({ id: 'bbbbbbbb-cccc-dddd-eeee-ffffffffffff', type: 'tick' }),
    ])[0];
    expect(groupRuns(siblings).map(r => r.id)).toEqual(['aaaaaaaa', 'bbbbbbbb']);
  });
});

describe('erno-devtools formatting', () => {
  it('formats clocks, uptime, and host names', () => {
    expect(formatClock(new Date(2026, 7, 25, 9, 5, 3))).toBe('09:05:03');
    expect(formatUptime(0, 65 * 60 * 1000)).toBe('1h05m');
    expect(formatMs(240)).toBe('240ms');
    expect(formatMs(null)).toBe('—');
    expect(apiHost('http://localhost:3000')).toBe('localhost:3000');
    expect(statusLabel('pending_retry')).toBe('Retrying');
    expect(statusTone('failed')).toBe('err');
    expect(syncLabel('synced')).toBe('in step');
  });
});
