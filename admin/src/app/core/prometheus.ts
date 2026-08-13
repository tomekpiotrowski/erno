import { HttpClient, HttpParams } from '@angular/common/http';
import { inject, Injectable } from '@angular/core';
import { map } from 'rxjs';

export interface PromPoint {
  t: number;
  v: number;
}

export interface PromSeries {
  metric: Record<string, string>;
  points: PromPoint[];
}

@Injectable({ providedIn: 'root' })
export class PrometheusService {
  private readonly http = inject(HttpClient);

  range(query: string, windowSeconds: number, step = '60s') {
    const end = Math.floor(Date.now() / 1000);
    const start = end - windowSeconds;
    const params = new HttpParams()
      .set('query', query)
      .set('start', start)
      .set('end', end)
      .set('step', step);
    return this.http.get<PromRangeResponse>('/prometheus/api/v1/query_range', { params }).pipe(
      map((res) =>
        (res.data?.result ?? []).map((r) => ({
          metric: r.metric,
          points: r.values.map(([t, v]) => ({ t: Number(t), v: Number(v) })),
        })),
      ),
    );
  }

  instant(query: string) {
    return this.http
      .get<PromInstantResponse>('/prometheus/api/v1/query', {
        params: new HttpParams().set('query', query),
      })
      .pipe(
        map((res) =>
          (res.data?.result ?? []).map((r) => ({
            metric: r.metric,
            value: Number(r.value[1]),
          })),
        ),
      );
  }
}

interface PromRangeResponse {
  data?: {
    result: { metric: Record<string, string>; values: [number, string][] }[];
  };
}

interface PromInstantResponse {
  data?: {
    result: { metric: Record<string, string>; value: [number, string] }[];
  };
}
