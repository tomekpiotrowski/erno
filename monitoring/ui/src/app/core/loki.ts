import { HttpClient, HttpParams } from '@angular/common/http';
import { inject, Injectable } from '@angular/core';
import { map } from 'rxjs';

export interface LogLine {
  ts: number;
  line: string;
  labels: Record<string, string>;
}

@Injectable({ providedIn: 'root' })
export class LokiService {
  private readonly http = inject(HttpClient);

  range(query: string, windowSeconds: number, limit = 200) {
    const endNs = Date.now() * 1e6;
    const startNs = endNs - windowSeconds * 1e9;
    const params = new HttpParams()
      .set('query', query)
      .set('start', String(Math.floor(startNs)))
      .set('end', String(Math.floor(endNs)))
      .set('limit', limit)
      .set('direction', 'backward');
    return this.http.get<LokiRangeResponse>('/loki/api/v1/query_range', { params }).pipe(
      map((res) => flatten(res)),
    );
  }
}

export function flatten(res: LokiRangeResponse): LogLine[] {
  const lines: LogLine[] = [];
  for (const stream of res.data?.result ?? []) {
    for (const [ts, line] of stream.values ?? []) {
      lines.push({
        ts: Number(ts) / 1e6,
        line,
        labels: stream.stream ?? {},
      });
    }
  }
  lines.sort((a, b) => b.ts - a.ts);
  return lines;
}

export function buildLogql(opts: {
  service?: string;
  level?: string;
  contains?: string;
  traceId?: string;
  raw?: string;
}): string {
  if (opts.raw?.trim()) return opts.raw.trim();
  const service = opts.service?.trim() || '.+';
  let q = `{service_name=~"${escapeLabel(service)}"}`;
  if (opts.level && opts.level !== 'all') {
    q += ` | severity_text="${opts.level.toUpperCase()}"`;
  }
  if (opts.contains?.trim()) {
    q += ` |= "${escapeFilter(opts.contains.trim())}"`;
  }
  if (opts.traceId?.trim()) {
    q += ` | trace_id="${escapeFilter(opts.traceId.trim())}"`;
  }
  return q;
}

function escapeLabel(value: string): string {
  return value.replace(/\\/g, '\\\\').replace(/"/g, '\\"');
}

function escapeFilter(value: string): string {
  return value.replace(/\\/g, '\\\\').replace(/"/g, '\\"');
}

interface LokiRangeResponse {
  data?: {
    result?: { stream?: Record<string, string>; values?: [string, string][] }[];
  };
}
