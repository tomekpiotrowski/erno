import { buildLogql, flatten, LokiService } from './loki';

describe('LokiService', () => {
  it('is provided in root', () => {
    expect(LokiService).toBeDefined();
  });
});

describe('buildLogql', () => {
  it('builds a bounded-label query from the form', () => {
    expect(buildLogql({})).toBe('{service_name=~".+"}');
    expect(buildLogql({ service: 'erno', level: 'error' })).toBe(
      '{service_name=~"erno"} | severity_text="ERROR"',
    );
    expect(buildLogql({ contains: 'timeout', traceId: 'abc' })).toBe(
      '{service_name=~".+"} |= "timeout" | trace_id="abc"',
    );
  });

  it('prefers a raw query when one is supplied', () => {
    expect(buildLogql({ raw: ' {job="api"} ', level: 'error' })).toBe('{job="api"}');
  });
});

describe('flatten', () => {
  it('flattens streams into newest-first lines', () => {
    expect(
      flatten({
        data: {
          result: [
            {
              stream: { service_name: 'erno' },
              values: [
                ['2000000000', 'second'],
                ['1000000000', 'first'],
              ],
            },
          ],
        },
      }),
    ).toEqual([
      { ts: 2000, line: 'second', labels: { service_name: 'erno' } },
      { ts: 1000, line: 'first', labels: { service_name: 'erno' } },
    ]);
  });

  it('survives a missing result array', () => {
    expect(flatten({})).toEqual([]);
  });
});
