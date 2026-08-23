import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { CollectorApi, UptimeList } from '../core/api';

// Docs: docs/src/content/docs/monitoring/uptime.md

@Component({
  selector: 'app-uptime',
  imports: [FormsModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="stack">
      <header class="head">
        <div>
          <h1>Uptime</h1>
          <p class="sub">
            Synthetic probes, run from this deployment. A check goes down only
            after consecutive failures, so one dropped packet does not raise an
            alarm.
          </p>
        </div>
      </header>

      <section class="panel">
        <header class="phead"><span class="eyebrow">Add a check</span></header>
        <div class="toolbar">
          <input [(ngModel)]="name" placeholder="Name, e.g. API liveness" />
          <input [(ngModel)]="url" placeholder="https://api.example.com/liveness" />
          <input
            [(ngModel)]="interval"
            type="number"
            min="10"
            placeholder="Interval (s)"
            style="max-width: 130px"
          />
          <button type="button" (click)="add()" [disabled]="!name() || !url()">Add</button>
        </div>
        @if (error(); as e) {
          <p class="error">{{ e }}</p>
        }
      </section>

      @if (data(); as d) {
        @if (!d.checks.length) {
          <section class="panel">
            <p class="muted">No checks yet.</p>
          </section>
        } @else {
          <section class="panel flush">
            <header class="phead">
              <span class="eyebrow">Checks</span>
              <span class="muted">last {{ d.window_hours }}h</span>
            </header>
            <table>
              <thead>
                <tr>
                  <th>State</th>
                  <th>Name</th>
                  <th>URL</th>
                  <th class="num">Uptime</th>
                  <th class="num">p50</th>
                  <th class="num">p95</th>
                  <th>Last checked</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                @for (c of d.checks; track c.id) {
                  <tr [class.flag]="c.state === 'down'">
                    <td><span [class]="pill(c.state)">{{ c.state }}</span></td>
                    <td>
                      {{ c.name }}
                      @if (!c.enabled) {
                        <span class="status warn">paused</span>
                      }
                    </td>
                    <td class="id">{{ c.method }} {{ c.url }}</td>
                    <td class="num">
                      {{ c.uptime_ratio === null ? '—' : (c.uptime_ratio * 100).toFixed(2) + '%' }}
                    </td>
                    <td class="num">{{ c.p50_ms ?? '—' }}</td>
                    <td class="num">{{ c.p95_ms ?? '—' }}</td>
                    <td class="id">{{ c.last_checked_at ?? 'never' }}</td>
                    <td>
                      <button type="button" class="ghost" (click)="toggle(c.id, !c.enabled)">
                        {{ c.enabled ? 'Pause' : 'Resume' }}
                      </button>
                      <button type="button" class="ghost" (click)="remove(c.id)">Delete</button>
                    </td>
                  </tr>
                }
              </tbody>
            </table>
          </section>
        }
      }
    </div>
  `,
})
export class UptimePage {
  private readonly api = inject(CollectorApi);

  data = signal<UptimeList | null>(null);
  name = signal('');
  url = signal('');
  interval = signal(60);
  error = signal<string | null>(null);

  constructor() {
    this.load();
    setInterval(() => this.load(), 15_000);
  }

  load() {
    this.api.uptime().subscribe((d) => this.data.set(d));
  }

  add() {
    this.error.set(null);
    this.api
      .createCheck({
        name: this.name(),
        url: this.url(),
        interval_seconds: Number(this.interval()) || 60,
      })
      .subscribe({
        next: () => {
          this.name.set('');
          this.url.set('');
          this.load();
        },
        error: (e: { error?: { error?: string } }) =>
          this.error.set(e.error?.error ?? 'Could not create the check.'),
      });
  }

  toggle(id: string, enabled: boolean) {
    this.api.setCheckEnabled(id, enabled).subscribe(() => this.load());
  }

  remove(id: string) {
    this.api.deleteCheck(id).subscribe(() => this.load());
  }

  pill(state: string): string {
    switch (state) {
      case 'up':
        return 'status good';
      case 'down':
        return 'status bad';
      default:
        return 'status info';
    }
  }
}
