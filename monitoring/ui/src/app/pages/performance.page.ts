import { Component, inject, signal, ChangeDetectionStrategy } from '@angular/core';
import { RouterLink } from '@angular/router';
import { PrometheusService, PromSeries } from '../core/prometheus';
import { TempoService, TraceHit } from '../core/tempo';
import { PERFORMANCE, SUBSYSTEMS, WINDOWS } from '../metrics/catalog';
import { Sparkline } from '../sparkline';

@Component({
  selector: 'app-performance',
  imports: [Sparkline, RouterLink],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="stack">
      <header class="head">
        <div>
          <h1>Performance</h1>
          <p class="sub">Request latency from Prometheus, plus slow traces from Tempo.</p>
        </div>
        <div class="toolbar">
          @for (w of windows; track w.id) {
            <button type="button" class="filter" [class.on]="windowId() === w.id" (click)="setWindow(w.id)">
              {{ w.id }}
            </button>
          }
        </div>
      </header>

      @if (error()) {
        <p class="error">{{ error() }}</p>
      }

      <section class="panel flush">
        <header class="phead"><span class="eyebrow">Slow traces</span></header>
        @if (traceError()) {
          <p class="error">{{ traceError() }}</p>
        } @else if (traces().length === 0) {
          <p class="muted">No traces slower than 500ms in this window.</p>
        } @else {
          <table>
            <thead>
              <tr><th>Trace</th><th>Name</th><th class="num">Duration</th></tr>
            </thead>
            <tbody>
              @for (t of traces(); track t.traceId) {
                <tr>
                  <td class="mono">
                    <a [routerLink]="['/performance/traces', t.traceId]">{{ t.traceId }}</a>
                  </td>
                  <td class="mono">{{ t.rootTraceName || t.rootServiceName || '—' }}</td>
                  <td class="num">{{ t.durationMs }} ms</td>
                </tr>
              }
            </tbody>
          </table>
        }
      </section>

      @for (block of blocks(); track block.title) {
        <section class="panel flush">
          <header class="phead"><span class="eyebrow">{{ block.title }}</span></header>
          <table>
            <thead>
              <tr><th>Series</th><th class="num">Latest</th><th></th></tr>
            </thead>
            <tbody>
              @for (s of block.series; track $index) {
                <tr>
                  <td class="mono">{{ label(s) }}</td>
                  <td class="num">{{ latest(s) }}</td>
                  <td><app-sparkline [points]="s.points" /></td>
                </tr>
              }
            </tbody>
          </table>
        </section>
      }
    </div>
  `,
})
export class PerformancePage {
  private readonly prom = inject(PrometheusService);
  private readonly tempo = inject(TempoService);
  windows = WINDOWS;
  windowId = signal<(typeof WINDOWS)[number]['id']>('1h');
  error = signal('');
  traceError = signal('');
  traces = signal<TraceHit[]>([]);
  blocks = signal<{ title: string; series: PromSeries[] }[]>([]);

  constructor() {
    this.load();
  }

  setWindow(id: (typeof WINDOWS)[number]['id']) {
    this.windowId.set(id);
    this.load();
  }

  label(s: PromSeries) {
    return Object.entries(s.metric)
      .map(([k, v]) => `${k}=${v}`)
      .join(' ') || '(no labels)';
  }

  latest(s: PromSeries) {
    return s.points.at(-1)?.v.toPrecision(4) ?? '—';
  }

  private load() {
    const w = WINDOWS.find((x) => x.id === this.windowId())!;
    this.error.set('');
    this.traceError.set('');
    this.tempo.search('{ duration > 500ms }', w.seconds).subscribe({
      next: (hits) => this.traces.set(hits),
      error: () =>
        this.traceError.set(
          'Tempo is unreachable. It runs in this deployment — check that it is up.',
        ),
    });
    const queries = [
      ['Request rate', PERFORMANCE.requestRate],
      ['5xx rate', PERFORMANCE.errorRate],
      ['p50 latency', PERFORMANCE.latencyP50],
      ['p95 latency', PERFORMANCE.latencyP95],
      ['p99 latency', PERFORMANCE.latencyP99],
      ['In-flight', PERFORMANCE.inFlight],
      ['DB pool total', PERFORMANCE.poolTotal],
      ['Job queue', PERFORMANCE.jobQueue],
      // Erno-specific timings — see metrics/catalog.ts.
      ['Job queue wait p95', SUBSYSTEMS.jobWaitP95],
      ['Job duration p95', SUBSYSTEMS.jobDurationP95],
      ['Job failure rate', SUBSYSTEMS.jobFailureRate],
      ['Sync delta p95', SUBSYSTEMS.syncDurationP95],
      ['Sync delta rows p95', SUBSYSTEMS.syncRowsP95],
      ['Storage upload p95', SUBSYSTEMS.storageUploadP95],
      ['Email send p95', SUBSYSTEMS.emailDurationP95],
    ] as const;
    const next: { title: string; series: PromSeries[] }[] = [];
    let remaining = queries.length;
    for (const [title, q] of queries) {
      this.prom.range(q, w.seconds, w.step).subscribe({
        next: (series) => {
          next.push({ title, series });
          remaining -= 1;
          if (remaining === 0) this.blocks.set(next);
        },
        error: () =>
          this.error.set(
            'Prometheus is unreachable. It runs in this deployment — check that it is up.',
          ),
      });
    }
  }
}
