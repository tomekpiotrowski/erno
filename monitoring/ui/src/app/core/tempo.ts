import { HttpClient, HttpParams } from '@angular/common/http';
import { inject, Injectable } from '@angular/core';
import { map } from 'rxjs';

export interface TraceHit {
  traceId: string;
  rootServiceName: string;
  rootTraceName: string;
  startTimeUnixNano: string;
  durationMs: number;
}

export interface TraceSpan {
  id: string;
  parentId: string;
  name: string;
  service: string;
  durationMs: number;
  startMs: number;
  status: string;
  attributes: Record<string, string>;
  children: TraceSpan[];
}

@Injectable({ providedIn: 'root' })
export class TempoService {
  private readonly http = inject(HttpClient);

  search(query: string, windowSeconds: number, limit = 20) {
    const end = Math.floor(Date.now() / 1000);
    const start = end - windowSeconds;
    const params = new HttpParams()
      .set('q', query)
      .set('start', start)
      .set('end', end)
      .set('limit', limit);
    return this.http.get<TempoSearchResponse>('/tempo/api/search', { params }).pipe(
      map((res) => (res.traces ?? []).map(toHit)),
    );
  }

  trace(id: string) {
    return this.http
      .get<TempoTraceResponse>(`/tempo/api/traces/${id}`, {
        headers: { Accept: 'application/json' },
      })
      .pipe(map((res) => toTree(res)));
  }
}

export function toHit(row: TempoSearchTrace): TraceHit {
  return {
    traceId: row.traceID ?? row.traceId ?? '',
    rootServiceName: row.rootServiceName ?? '',
    rootTraceName: row.rootTraceName ?? '',
    startTimeUnixNano: String(row.startTimeUnixNano ?? ''),
    durationMs: Number(row.durationMs ?? 0),
  };
}

export function toTree(res: TempoTraceResponse): TraceSpan[] {
  const flat: Omit<TraceSpan, 'children'>[] = [];
  for (const batch of res.batches ?? []) {
    const service =
      otelString(
        (batch.resource?.attributes ?? []).find((a) => a.key === 'service.name'),
      ) || '';
    for (const scope of batch.scopeSpans ?? batch.instrumentationLibrarySpans ?? []) {
      for (const span of scope.spans ?? []) {
        const start = nanoToMs(span.startTimeUnixNano);
        const end = nanoToMs(span.endTimeUnixNano);
        flat.push({
          id: hex(span.spanId),
          parentId: hex(span.parentSpanId),
          name: span.name ?? '(span)',
          service,
          durationMs: Math.max(0, end - start),
          startMs: start,
          status: statusLabel(span.status?.code),
          attributes: Object.fromEntries(
            (span.attributes ?? [])
              .map((a) => [a.key, otelString(a)] as const)
              .filter(([, v]) => v !== ''),
          ),
        });
      }
    }
  }
  return nest(flat);
}

function nest(flat: Omit<TraceSpan, 'children'>[]): TraceSpan[] {
  const byId = new Map<string, TraceSpan>();
  for (const s of flat) {
    byId.set(s.id, { ...s, children: [] });
  }
  const roots: TraceSpan[] = [];
  for (const node of byId.values()) {
    const parent = node.parentId ? byId.get(node.parentId) : undefined;
    if (parent) {
      parent.children.push(node);
    } else {
      roots.push(node);
    }
  }
  const sort = (nodes: TraceSpan[]) => {
    nodes.sort((a, b) => a.startMs - b.startMs);
    for (const n of nodes) sort(n.children);
  };
  sort(roots);
  return roots;
}

function nanoToMs(value: string | number | undefined): number {
  if (value === undefined || value === '') return 0;
  const n = typeof value === 'number' ? value : Number(value);
  if (!Number.isFinite(n)) return 0;
  return n / 1e6;
}

function hex(id: string | undefined): string {
  return (id ?? '').toLowerCase();
}

function statusLabel(code: number | string | undefined): string {
  if (code === 2 || code === 'STATUS_CODE_ERROR' || code === 'ERROR') return 'error';
  if (code === 1 || code === 'STATUS_CODE_OK' || code === 'OK') return 'ok';
  return 'unset';
}

function otelString(attr: OtelAttribute | undefined): string {
  if (!attr) return '';
  const v = attr.value;
  if (!v) return '';
  if (typeof v.stringValue === 'string') return v.stringValue;
  if (typeof v.intValue === 'string' || typeof v.intValue === 'number') return String(v.intValue);
  if (typeof v.doubleValue === 'number') return String(v.doubleValue);
  if (typeof v.boolValue === 'boolean') return String(v.boolValue);
  return '';
}

interface TempoSearchTrace {
  traceID?: string;
  traceId?: string;
  rootServiceName?: string;
  rootTraceName?: string;
  startTimeUnixNano?: string | number;
  durationMs?: number;
}

interface TempoSearchResponse {
  traces?: TempoSearchTrace[];
}

interface OtelAttribute {
  key: string;
  value?: {
    stringValue?: string;
    intValue?: string | number;
    doubleValue?: number;
    boolValue?: boolean;
  };
}

interface TempoTraceResponse {
  batches?: {
    resource?: { attributes?: OtelAttribute[] };
    scopeSpans?: { spans?: OtelSpan[] }[];
    instrumentationLibrarySpans?: { spans?: OtelSpan[] }[];
  }[];
}

interface OtelSpan {
  spanId?: string;
  parentSpanId?: string;
  name?: string;
  startTimeUnixNano?: string | number;
  endTimeUnixNano?: string | number;
  status?: { code?: number | string };
  attributes?: OtelAttribute[];
}
