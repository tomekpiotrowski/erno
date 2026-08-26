import { DevJob } from './erno-dev-jobs.service';
import {
  apiHost,
  decodeJwtClaims,
  downloadJson,
  filterJobGroups,
  formatClock,
  formatMs,
  formatUptime,
  groupJobs,
  groupRuns,
  parseEmailAuthLink,
  prependPushEvent,
  statusLabel,
  statusTone,
  syncLabel,
  tokenFingerprint,
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

function jwt(payload: object): string {
  const json = btoa(JSON.stringify(payload))
    .replace(/=+$/, '')
    .replace(/\+/g, '-')
    .replace(/\//g, '_');
  return `eyJhbGciOiJub25lIn0.${json}.sig`;
}

describe('erno-devtools JWT claims', () => {
  it('decodes sub, ver, exp, and iat from an access token', () => {
    const claims = decodeJwtClaims(jwt({ sub: 'user-1', ver: 3, exp: 1700000000, iat: 1699999900 }));
    expect(claims).toEqual({ sub: 'user-1', ver: 3, exp: 1700000000, iat: 1699999900 });
  });

  it('returns null for missing or malformed tokens', () => {
    expect(decodeJwtClaims(null)).toBeNull();
    expect(decodeJwtClaims('not-a-jwt')).toBeNull();
    expect(decodeJwtClaims('a.!!!')).toBeNull();
  });
});

describe('erno-devtools email auth links', () => {
  it('extracts a verify-email token from HTML', () => {
    const html = '<p>Click <a href="http://localhost:4200/verify-email?token=abc%2B12">here</a></p>';
    expect(parseEmailAuthLink(html)).toEqual({
      kind: 'verify',
      token: 'abc+12',
      url: 'http://localhost:4200/verify-email?token=abc%2B12',
    });
  });

  it('extracts a reset-password token from plain text', () => {
    const text = 'Paste: https://app.example.com/reset-password?token=rst_99';
    expect(parseEmailAuthLink(text)).toEqual({
      kind: 'reset',
      token: 'rst_99',
      url: 'https://app.example.com/reset-password?token=rst_99',
    });
  });

  it('returns null when the body has no auth link', () => {
    expect(parseEmailAuthLink('<p>Weekly digest</p>')).toBeNull();
    expect(parseEmailAuthLink(null)).toBeNull();
  });
});

describe('erno-devtools token fingerprint', () => {
  it('is a checksum, not a prefix of the token', () => {
    const token = 'refresh-secret-value-abcdefghijklmnopqrstuvwxyz';
    const fp = tokenFingerprint(token);
    expect(fp).toMatch(/^[0-9a-f]{8}$/);
    expect(token.replace(/-/g, '').startsWith(fp)).toBe(false);
    expect(tokenFingerprint(token)).toBe(fp);
    expect(tokenFingerprint('other-secret-value-abcdefghijklmnopqrstuvwxyz')).not.toBe(fp);
  });

  it('returns missing for an empty token', () => {
    expect(tokenFingerprint(null)).toBe('missing');
  });
});

describe('erno-devtools push log', () => {
  it('prepends events with a receive clock and caps at 30', () => {
    const event = { entity: 'todos', id: 'a', sync_seq: 1, deleted: false };
    const one = prependPushEvent([], event, 1_700_000_000_000);
    expect(one[0]).toEqual({ ...event, at: 1_700_000_000_000 });
    let list = one;
    for (let i = 0; i < 40; i++) {
      list = prependPushEvent(list, { ...event, sync_seq: i + 2 }, i);
    }
    expect(list).toHaveLength(30);
    expect(list[0].sync_seq).toBe(41);
  });
});

describe('erno-devtools downloadJson', () => {
  it('triggers a file download with the JSON payload', () => {
    const click = vi.fn();
    const a = document.createElement('a');
    a.click = click;
    const createSpy = vi.spyOn(document, 'createElement').mockReturnValue(a);
    const createObjectURL = vi.fn().mockReturnValue('blob:test');
    const revokeObjectURL = vi.fn();
    const origCreate = URL.createObjectURL;
    const origRevoke = URL.revokeObjectURL;
    URL.createObjectURL = createObjectURL;
    URL.revokeObjectURL = revokeObjectURL;

    try {
      downloadJson('erno-syncMeta.json', [{ entity: 'todos', lastSyncSeq: 4 }]);
      expect(createObjectURL).toHaveBeenCalled();
      expect(a.download).toBe('erno-syncMeta.json');
      expect(click).toHaveBeenCalled();
      expect(revokeObjectURL).toHaveBeenCalledWith('blob:test');
    } finally {
      createSpy.mockRestore();
      URL.createObjectURL = origCreate;
      URL.revokeObjectURL = origRevoke;
    }
  });
});
