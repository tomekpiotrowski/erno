import { TestBed } from '@angular/core/testing';
import { provideHttpClient } from '@angular/common/http';
import {
  HttpTestingController,
  provideHttpClientTesting,
} from '@angular/common/http/testing';
import { PrometheusService } from './prometheus';

describe('PrometheusService', () => {
  let prom: PrometheusService;
  let http: HttpTestingController;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [provideHttpClient(), provideHttpClientTesting()],
    });
    prom = TestBed.inject(PrometheusService);
    http = TestBed.inject(HttpTestingController);
  });

  afterEach(() => http.verify());

  it('turns a range response into series of numeric points', () => {
    let series: unknown;
    prom.range('up', 3600).subscribe((s) => (series = s));

    const req = http.expectOne((r) => r.url === '/prometheus/api/v1/query_range');
    // Prometheus returns sample values as strings; the mapping must not leave
    // them that way or the sparkline plots NaN.
    req.flush({
      data: {
        result: [
          {
            metric: { table: 'error_event' },
            values: [
              [1000, '1.5'],
              [1060, '2'],
            ],
          },
        ],
      },
    });

    expect(series).toEqual([
      {
        metric: { table: 'error_event' },
        points: [
          { t: 1000, v: 1.5 },
          { t: 1060, v: 2 },
        ],
      },
    ]);
  });

  it('sends the query window as start, end and step', () => {
    prom.range('up', 600, '30s').subscribe();

    const req = http.expectOne((r) => r.url === '/prometheus/api/v1/query_range');
    expect(req.request.params.get('query')).toBe('up');
    expect(req.request.params.get('step')).toBe('30s');

    const start = Number(req.request.params.get('start'));
    const end = Number(req.request.params.get('end'));
    expect(end - start).toBe(600);

    req.flush({ data: { result: [] } });
  });

  it('reads the value out of an instant response', () => {
    let result: unknown;
    prom.instant('up').subscribe((r) => (result = r));

    http
      .expectOne((r) => r.url === '/prometheus/api/v1/query')
      .flush({ data: { result: [{ metric: { job: 'api' }, value: [1000, '3'] }] } });

    expect(result).toEqual([{ metric: { job: 'api' }, value: 3 }]);
  });

  it('yields an empty list when Prometheus returns no data', () => {
    // Prometheus omits `data` entirely on some error shapes, and a monitoring
    // console that throws when its own backend is unhappy is the wrong outcome.
    let range: unknown;
    let instant: unknown;
    prom.range('up', 60).subscribe((s) => (range = s));
    http.expectOne((r) => r.url === '/prometheus/api/v1/query_range').flush({});
    prom.instant('up').subscribe((s) => (instant = s));
    http.expectOne((r) => r.url === '/prometheus/api/v1/query').flush({});

    expect(range).toEqual([]);
    expect(instant).toEqual([]);
  });
});
