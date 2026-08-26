import { TempoService, toHit, toTree } from './tempo';

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
  });
});

describe('TempoService', () => {
  it('is provided in root', () => {
    expect(TempoService).toBeDefined();
  });
});
