import { Component, inject, signal } from '@angular/core';
import { PrometheusService, PromPoint } from '../core/prometheus';
import { BUSINESS, WINDOWS } from '../metrics/catalog';
import { Sparkline } from '../sparkline';

@Component({
  selector: 'app-business',
  imports: [Sparkline],
  template: `
    <div class="stack">
      <header class="head">
        <div>
          <h1>Statistics</h1>
          <p class="sub">Business figures from Prometheus. Every number states its window.</p>
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

      <div class="grid cards">
        @for (row of cards(); track row.label) {
          <div class="stat">
            <span class="label">{{ row.label }}</span>
            <span class="value">{{ row.value }}</span>
            <app-sparkline [points]="row.points" />
          </div>
        }
      </div>
    </div>
  `,
})
export class BusinessPage {
  private readonly prom = inject(PrometheusService);
  windows = WINDOWS;
  windowId = signal<(typeof WINDOWS)[number]['id']>('24h');
  error = signal('');
  cards = signal<{ label: string; value: string; points: PromPoint[] }[]>([]);

  constructor() {
    this.load();
  }

  setWindow(id: (typeof WINDOWS)[number]['id']) {
    this.windowId.set(id);
    this.load();
  }

  private load() {
    const w = WINDOWS.find((x) => x.id === this.windowId())!;
    this.error.set('');
    const queries: [string, string][] = [
      ['Users', BUSINESS.users],
      ['Paid', BUSINESS.paid],
      ['Trial', BUSINESS.trial],
      ['Gift', BUSINESS.gift],
      ['Active 1d', BUSINESS.active1d],
      ['Active 7d', BUSINESS.active7d],
      ['Signups / 24h', BUSINESS.registered],
      ['Deletes / 24h', BUSINESS.deleted],
      ['Solves / 24h', BUSINESS.cubeastSolves],
      ['Sessions / 24h', BUSINESS.cubeastSessions],
    ];
    const next: { label: string; value: string; points: PromPoint[] }[] = [];
    let remaining = queries.length;
    for (const [label, q] of queries) {
      this.prom.range(q, w.seconds, w.step).subscribe({
        next: (series) => {
          const points = series[0]?.points ?? [];
          const last = points.at(-1)?.v;
          if (last !== undefined && !Number.isNaN(last)) {
            next.push({ label, value: last.toPrecision(4), points });
          }
          remaining -= 1;
          if (remaining === 0) this.cards.set(next);
        },
        error: () => this.error.set('Prometheus is unreachable. Is it running?'),
      });
    }
  }
}
