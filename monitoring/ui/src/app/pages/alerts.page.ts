import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { AlertRule, CollectorApi } from '../core/api';

// Docs: docs/src/content/docs/monitoring/alerts.md

@Component({
  selector: 'app-alerts',
  imports: [FormsModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="stack">
      <header class="head">
        <div>
          <h1>Alerts</h1>
          <p class="sub">
            Rules are held for a while before they fire and repeat on a schedule
            rather than on every evaluation, so a blip does not wake anyone.
          </p>
        </div>
      </header>

      <section class="panel">
        <header class="phead"><span class="eyebrow">Add a rule</span></header>
        <div class="toolbar">
          <input [(ngModel)]="name" placeholder="Name, e.g. New error types" />
          <select [(ngModel)]="source" (ngModelChange)="onSourceChange($event)">
            <option value="errors">errors</option>
            <option value="uptime">uptime</option>
            <option value="subsystem">subsystem</option>
            <option value="promql">promql</option>
          </select>
          <!-- A PromQL query is free text and long, so it gets an input rather
               than the fixed selector list the other sources use. -->
          @if (source() === 'promql') {
            <input
              [(ngModel)]="selector"
              placeholder="PromQL, e.g. rate(http_requests_total{status=~&quot;5..&quot;}[5m])"
              style="flex: 1 1 24rem"
            />
          } @else {
            <select [(ngModel)]="selector">
              @for (s of selectorsFor(source()); track s.value) {
                <option [value]="s.value">{{ s.label }}</option>
              }
            </select>
          }
        </div>
        <div class="toolbar">
          <select [(ngModel)]="comparator">
            <option value="gt">greater than</option>
            <option value="gte">at least</option>
            <option value="lt">less than</option>
          </select>
          <input [(ngModel)]="threshold" type="number" style="max-width: 110px" />
          <input
            [(ngModel)]="forSeconds"
            type="number"
            placeholder="hold (s)"
            style="max-width: 130px"
          />
          <input [(ngModel)]="email" placeholder="Notify email (optional)" />
          <button type="button" (click)="add()" [disabled]="!name()">Add</button>
        </div>
        @if (error(); as e) {
          <p class="error">{{ e }}</p>
        }
      </section>

      @if (rules().length) {
        <section class="panel flush">
          <header class="phead"><span class="eyebrow">Rules</span></header>
          <table>
            <thead>
              <tr>
                <th>State</th>
                <th>Name</th>
                <th>Condition</th>
                <th>Last reading</th>
                <th>Notify</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              @for (r of rules(); track r.id) {
                <tr [class.flag]="r.state === 'firing'">
                  <td>
                    <span [class]="pill(r.state)">{{ r.state }}</span>
                    @if (r.silence_until) {
                      <span class="status warn">silenced</span>
                    }
                    @if (!r.enabled) {
                      <span class="status warn">off</span>
                    }
                  </td>
                  <td>{{ r.name }}</td>
                  <td class="mono">
                    {{ r.source }}{{ r.selector ? '/' + r.selector : '' }}
                    {{ symbol(r.comparator) }} {{ r.threshold }}
                    @if (r.for_seconds > 0) {
                      <span class="muted">for {{ r.for_seconds }}s</span>
                    }
                  </td>
                  <td class="muted">{{ r.last_value ?? 'not evaluated yet' }}</td>
                  <td class="id">{{ r.notify_email ?? '—' }}</td>
                  <td>
                    <button type="button" class="ghost" (click)="silence(r.id)">Silence 1h</button>
                    <button type="button" class="ghost" (click)="toggle(r.id, !r.enabled)">
                      {{ r.enabled ? 'Disable' : 'Enable' }}
                    </button>
                    <button type="button" class="ghost" (click)="remove(r.id)">Delete</button>
                  </td>
                </tr>
              }
            </tbody>
          </table>
        </section>
      } @else {
        <section class="panel">
          <p class="muted">
            No rules yet. A good first one: source <code>errors</code>, selector
            <code>new_issues</code>, greater than 0 — tells you the first time an
            error type is ever seen.
          </p>
        </section>
      }
    </div>
  `,
  styles: `select { max-width: 190px; }`,
})
export class AlertsPage {
  private readonly api = inject(CollectorApi);

  rules = signal<AlertRule[]>([]);
  name = signal('');
  source = signal('errors');
  selector = signal('new_issues');
  comparator = signal('gt');
  threshold = signal(0);
  forSeconds = signal(120);
  email = signal('');
  error = signal<string | null>(null);

  constructor() {
    this.load();
    setInterval(() => this.load(), 20_000);
  }

  load() {
    this.api.alertRules().subscribe((r) => this.rules.set(r.rules));
  }

  selectorsFor(source: string) {
    switch (source) {
      case 'errors':
        return [
          { value: 'new_issues', label: 'new error types' },
          { value: 'all', label: 'all events' },
          { value: 'api', label: 'events from the API' },
          { value: 'app', label: 'events from apps' },
          { value: 'admin', label: 'events from the admin panel' },
        ];
      case 'uptime':
        return [{ value: 'all', label: 'any check down' }];
      default:
        return [
          { value: 'down', label: 'instances down' },
          { value: 'degraded', label: 'instances not healthy' },
        ];
    }
  }

  onSourceChange(source: string) {
    // promql takes a free-text query, so it starts empty rather than inheriting
    // the first entry of a fixed list that does not apply to it.
    this.selector.set(source === 'promql' ? '' : this.selectorsFor(source)[0].value);
  }

  add() {
    this.error.set(null);
    this.api
      .createAlertRule({
        name: this.name(),
        source: this.source(),
        selector: this.selector(),
        comparator: this.comparator(),
        threshold: Number(this.threshold()),
        for_seconds: Number(this.forSeconds()),
        notify_email: this.email() || undefined,
      })
      .subscribe({
        next: () => {
          this.name.set('');
          this.email.set('');
          this.load();
        },
        error: (e: { error?: { error?: string } }) =>
          this.error.set(e.error?.error ?? 'Could not create the rule.'),
      });
  }

  toggle(id: string, enabled: boolean) {
    this.api.setAlertRuleEnabled(id, enabled).subscribe(() => this.load());
  }

  silence(id: string) {
    this.api.silenceAlertRule(id, 60).subscribe(() => this.load());
  }

  remove(id: string) {
    this.api.deleteAlertRule(id).subscribe(() => this.load());
  }

  symbol(comparator: string) {
    return { gt: '>', gte: '≥', lt: '<', lte: '≤' }[comparator] ?? '>';
  }

  pill(state: string) {
    switch (state) {
      case 'firing':
        return 'status bad';
      case 'pending':
        return 'status warn';
      default:
        return 'status good';
    }
  }
}
