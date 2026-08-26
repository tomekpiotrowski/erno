import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { ActivatedRoute } from '@angular/router';
import { FormsModule } from '@angular/forms';
import { WINDOWS } from '../metrics/catalog';
import { LokiService, LogLine, buildLogql } from '../core/loki';

@Component({
  selector: 'app-logs',
  imports: [FormsModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="stack">
      <header class="head">
        <div>
          <h1>Logs</h1>
          <p class="sub">Lines from Loki. This is grep, not issue grouping — see Issues for that.</p>
        </div>
        <div class="toolbar">
          @for (w of windows; track w.id) {
            <button type="button" class="filter" [class.on]="windowId() === w.id" (click)="setWindow(w.id)">
              {{ w.id }}
            </button>
          }
        </div>
      </header>

      <section class="panel">
        <form class="filters" (submit)="$event.preventDefault(); load()">
          <label>
            Service
            <input type="text" [ngModel]="service()" (ngModelChange)="service.set($event)" name="service" placeholder="erno" />
          </label>
          <label>
            Level
            <select [ngModel]="level()" (ngModelChange)="level.set($event)" name="level">
              <option value="all">all</option>
              <option value="error">error</option>
              <option value="warn">warn</option>
              <option value="info">info</option>
            </select>
          </label>
          <label>
            Contains
            <input type="text" [ngModel]="contains()" (ngModelChange)="contains.set($event)" name="contains" />
          </label>
          <label class="grow">
            LogQL
            <input type="text" [ngModel]="raw()" (ngModelChange)="raw.set($event)" name="raw" placeholder="leave blank to build from the filters" />
          </label>
          <button type="submit">Query</button>
        </form>
        <p class="muted mono">{{ query() }}</p>
      </section>

      @if (error()) {
        <p class="error">{{ error() }}</p>
      }

      <section class="panel flush">
        <header class="phead"><span class="eyebrow">{{ lines().length }} lines</span></header>
        <table>
          <thead>
            <tr><th>When</th><th>Service</th><th>Line</th></tr>
          </thead>
          <tbody>
            @for (l of lines(); track $index) {
              <tr>
                <td class="mono">{{ when(l.ts) }}</td>
                <td class="mono">{{ l.labels['service_name'] ?? '—' }}</td>
                <td class="mono">{{ l.line }}</td>
              </tr>
            }
          </tbody>
        </table>
      </section>
    </div>
  `,
  styles: `
    .filters {
      display: flex;
      flex-wrap: wrap;
      gap: 12px;
      align-items: end;
      padding: 12px;
    }
    .filters label { display: flex; flex-direction: column; gap: 4px; font-size: 12px; color: var(--cb-fg-dim); }
    .filters .grow { flex: 1 1 240px; }
    .filters input, .filters select {
      background: var(--cb-elev2);
      border: 1px solid var(--cb-border);
      color: var(--cb-fg);
      padding: 6px 8px;
      font-family: var(--cb-font-mono);
    }
  `,
})
export class LogsPage {
  private readonly loki = inject(LokiService);
  private readonly route = inject(ActivatedRoute);
  windows = WINDOWS;
  windowId = signal<(typeof WINDOWS)[number]['id']>('1h');
  service = signal('');
  level = signal('all');
  contains = signal('');
  raw = signal('');
  lines = signal<LogLine[]>([]);
  error = signal('');
  query = signal('');

  constructor() {
    const trace = this.route.snapshot.queryParamMap.get('trace');
    if (trace) this.raw.set(buildLogql({ traceId: trace }));
    this.load();
  }

  setWindow(id: (typeof WINDOWS)[number]['id']) {
    this.windowId.set(id);
    this.load();
  }

  load() {
    const w = WINDOWS.find((x) => x.id === this.windowId())!;
    const q = buildLogql({
      service: this.service(),
      level: this.level(),
      contains: this.contains(),
      raw: this.raw(),
    });
    this.query.set(q);
    this.error.set('');
    this.loki.range(q, w.seconds).subscribe({
      next: (lines) => this.lines.set(lines),
      error: () =>
        this.error.set(
          'Loki is unreachable. It runs in this deployment — check that it is up.',
        ),
    });
  }

  when(ts: number) {
    return new Date(ts).toISOString();
  }
}
