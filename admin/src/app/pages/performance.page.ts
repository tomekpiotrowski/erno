import { Component, inject, signal } from '@angular/core';
import { PrometheusService, PromSeries } from '../core/prometheus';
import { PERFORMANCE, WINDOWS } from '../metrics/catalog';
import { Sparkline } from '../sparkline';

@Component({
  selector: 'app-performance',
  imports: [Sparkline],
  template: `
    <div class="stack">
      <header class="head">
        <div>
          <h1>Performance</h1>
          <p class="sub">Request latency and queue depth from Prometheus.</p>
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
  windows = WINDOWS;
  windowId = signal<(typeof WINDOWS)[number]['id']>('1h');
  error = signal('');
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
    const queries = [
      ['Request rate', PERFORMANCE.requestRate],
      ['5xx rate', PERFORMANCE.errorRate],
      ['p50 latency', PERFORMANCE.latencyP50],
      ['p95 latency', PERFORMANCE.latencyP95],
      ['p99 latency', PERFORMANCE.latencyP99],
      ['In-flight', PERFORMANCE.inFlight],
      ['DB pool total', PERFORMANCE.poolTotal],
      ['Job queue', PERFORMANCE.jobQueue],
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
        error: () => this.error.set('Prometheus is unreachable. Is it running?'),
      });
    }
  }
}
