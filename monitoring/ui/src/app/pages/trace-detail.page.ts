import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { n1Insight, TempoService, TraceSpan } from '../core/tempo';
import { LokiService, LogLine, buildLogql } from '../core/loki';

@Component({
  selector: 'app-trace-detail',
  imports: [RouterLink],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="stack">
      <header class="head">
        <div>
          <a routerLink="/performance" class="muted">← Performance</a>
          <h1>Trace</h1>
          <p class="sub mono">{{ id }}</p>
        </div>
      </header>

      @if (error()) {
        <p class="error">{{ error() }}</p>
      }

      <section class="panel">
        <header class="phead"><span class="eyebrow">Spans</span></header>
        @if (rows().length === 0 && !error()) {
          <p class="muted">No spans in this trace.</p>
        } @else {
          <div class="tree">
            @for (row of rows(); track row.span.id) {
              <div class="span" [style.paddingLeft.px]="row.depth * 16">
                <div class="span-head">
                  <span class="mono">{{ row.span.name }}</span>
                  <span class="muted">{{ row.span.service }}</span>
                  <span class="num">{{ row.span.durationMs.toFixed(1) }} ms</span>
                  <span [class]="row.span.status === 'error' ? 'error' : 'muted'">{{ row.span.status }}</span>
                </div>
                <div class="bar-track">
                  <div class="bar" [style.width.%]="bar(row.span.durationMs)"></div>
                </div>
                @if (attrList(row.span); as attrs) {
                  @if (attrs) {
                    <p class="muted mono attrs">{{ attrs }}</p>
                  }
                }
                @for (ev of row.span.events; track $index) {
                  @if (ev.attributes['db.statement'] || ev.name) {
                    <p class="muted mono attrs">{{ ev.attributes['db.statement'] || ev.name }}</p>
                  }
                }
              </div>
            }
          </div>
        }
        @if (insight()) {
          <p class="muted">↳ {{ insight() }}</p>
        }
      </section>

      <section class="panel flush">
        <header class="phead"><span class="eyebrow">Logs for this trace</span></header>
        @if (logError()) {
          <p class="error">{{ logError() }}</p>
        } @else if (logs().length === 0) {
          <p class="muted">No log lines with this trace id in the last hour.</p>
        } @else {
          <table>
            <thead>
              <tr><th>When</th><th>Line</th></tr>
            </thead>
            <tbody>
              @for (l of logs(); track $index) {
                <tr>
                  <td class="mono">{{ when(l.ts) }}</td>
                  <td class="mono">{{ l.line }}</td>
                </tr>
              }
            </tbody>
          </table>
        }
      </section>
    </div>
  `,
  styles: `
    .tree { display: flex; flex-direction: column; gap: 8px; padding: 12px; }
    .span-head { display: flex; gap: 12px; align-items: baseline; flex-wrap: wrap; }
    .bar-track { height: 4px; background: var(--cb-track); margin-top: 4px; }
    .bar { height: 4px; background: var(--cb-accent); }
    .attrs { margin: 4px 0 0; }
  `,
})
export class TraceDetailPage {
  private readonly tempo = inject(TempoService);
  private readonly loki = inject(LokiService);
  readonly id = inject(ActivatedRoute).snapshot.paramMap.get('id') ?? '';
  roots = signal<TraceSpan[]>([]);
  logs = signal<LogLine[]>([]);
  error = signal('');
  logError = signal('');
  rows = computed(() => flatten(this.roots()));
  insight = computed(() => n1Insight(this.roots()));

  constructor() {
    if (!this.id) {
      this.error.set('Missing trace id.');
      return;
    }
    this.tempo.trace(this.id).subscribe({
      next: (roots) => this.roots.set(roots),
      error: () =>
        this.error.set(
          'Tempo is unreachable. It runs in this deployment — check that it is up.',
        ),
    });
    this.loki.range(buildLogql({ traceId: this.id }), 3600).subscribe({
      next: (lines) => this.logs.set(lines),
      error: () =>
        this.logError.set(
          'Loki is unreachable. It runs in this deployment — check that it is up.',
        ),
    });
  }

  bar(ms: number) {
    const root = Math.max(1, ...this.roots().map((s) => s.durationMs));
    return Math.min(100, (ms / root) * 100);
  }

  attrList(span: TraceSpan) {
    return Object.entries(span.attributes)
      .map(([k, v]) => `${k}=${v}`)
      .join('  ');
  }

  when(ts: number) {
    return new Date(ts).toISOString();
  }
}

function flatten(nodes: TraceSpan[], depth = 0): { span: TraceSpan; depth: number }[] {
  const out: { span: TraceSpan; depth: number }[] = [];
  for (const n of nodes) {
    out.push({ span: n, depth });
    out.push(...flatten(n.children, depth + 1));
  }
  return out;
}
