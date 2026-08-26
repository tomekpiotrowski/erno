import { n1Insight, TempoService, toHit, toTree } from './tempo';

describe('toHit', () => {
  it('maps a search row into a typed hit', () => {
    expect(
      toHit({
        traceID: 'abc',
        rootServiceName: 'erno',
        rootTraceName: 'GET /widgets/{id}',
        startTimeUnixNano: '1',
        durationMs: 812,
      }),
    ).toEqual({
      traceId: 'abc',
      rootServiceName: 'erno',
      rootTraceName: 'GET /widgets/{id}',
      startTimeUnixNano: '1',
      durationMs: 812,
    });
  });
});

describe('toTree', () => {
  it('nests spans by parent into a tree', () => {
    const tree = toTree({
      batches: [
        {
          resource: {
            attributes: [{ key: 'service.name', value: { stringValue: 'erno' } }],
          },
          scopeSpans: [
            {
              spans: [
                {
                  spanId: 'aa',
                  name: 'GET /widgets/{id}',
                  startTimeUnixNano: '1000000000',
                  endTimeUnixNano: '2000000000',
                  status: { code: 1 },
                },
                {
                  spanId: 'bb',
                  parentSpanId: 'aa',
                  name: 'sync',
                  startTimeUnixNano: '1100000000',
                  endTimeUnixNano: '1500000000',
                  status: { code: 2 },
                  attributes: [{ key: 'kind', value: { stringValue: 'deck' } }],
                },
              ],
            },
          ],
        },
      ],
    });
    expect(tree).toHaveLength(1);
    expect(tree[0].name).toBe('GET /widgets/{id}');
    expect(tree[0].status).toBe('ok');
    expect(tree[0].children).toHaveLength(1);
    expect(tree[0].children[0].name).toBe('sync');
    expect(tree[0].children[0].status).toBe('error');
    expect(tree[0].children[0].attributes['kind']).toBe('deck');
    expect(tree[0].durationMs).toBe(1000);
    expect(tree[0].events).toEqual([]);
  });

  it('keeps sqlx events and reports N+1', () => {
    const events = Array.from({ length: 8 }, () => ({
      name: 'query',
      attributes: [{ key: 'db.statement', value: { stringValue: 'SELECT 1' } }],
    }));
    const tree = toTree({
      batches: [
        {
          scopeSpans: [
            {
              spans: [
                {
                  spanId: 'aa',
                  name: 'GET /x',
                  startTimeUnixNano: '1',
                  endTimeUnixNano: '2',
                  events,
                },
              ],
            },
          ],
        },
      ],
    });
    expect(tree[0].events).toHaveLength(8);
    expect(n1Insight(tree)).toMatch(/8 similar/);
  });
});

describe('TempoService', () => {
  it('is provided in root', () => {
    expect(TempoService).toBeDefined();
  });
});
