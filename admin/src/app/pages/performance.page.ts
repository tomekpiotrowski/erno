import { Component, inject, signal } from '@angular/core';
import { PrometheusService, PromSeries } from '../core/prometheus';
import { PERFORMANCE, WINDOWS } from '../metrics/catalog';
import { Sparkline } from '../sparkline';

@Component({
  selector: 'app-performance',
  imports: [Sparkline],
  template: `
    <h1>Performance</h1>
    <div class="toolbar">
      @for (w of windows; track w.id) {
        <button (click)="setWindow(w.id)">{{ w.id }}</button>
      }
    </div>
    @if (error()) {
      <p class="error">{{ error() }}</p>
    }
    @for (block of blocks(); track block.title) {
      <h2>{{ block.title }}</h2>
      <table>
        <tr><th>Series</th><th>Latest</th><th></th></tr>
        @for (s of block.series; track $index) {
          <tr>
            <td>{{ label(s) }}</td>
            <td>{{ latest(s) }}</td>
            <td><app-sparkline [points]="s.points" /></td>
          </tr>
        }
      </table>
    }
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
