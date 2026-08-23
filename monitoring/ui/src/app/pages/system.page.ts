import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { CollectorApi, HealthResponse, HealthState } from '../core/api';

// Docs: docs/src/content/docs/monitoring/subsystem-health.md

@Component({
  selector: 'app-system',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="stack">
      <header class="head">
        <div>
          <h1>System</h1>
          <p class="sub">
            What each application instance reports about its own subsystems.
            A heartbeat that stops is itself the signal.
          </p>
        </div>
        @if (data(); as d) {
          <span [class]="pill(d.state)">{{ d.state }}</span>
        }
      </header>

      @if (data(); as d) {
        @if (!d.instances.length) {
          <section class="panel">
            <p class="muted">
              No instance has reported yet. An application reports when
              <code>[error_reporting]</code> has a <code>collector_url</code> and
              <code>report_health</code> is on.
            </p>
          </section>
        } @else {
          @for (i of d.instances; track i.instance) {
            <section class="panel">
              <header class="phead">
                <span class="eyebrow">{{ i.instance }}</span>
                <span [class]="pill(i.state)">{{ i.state }}</span>
              </header>

              <dl class="kv">
                <dt>Environment</dt>
                <dd>{{ i.environment }}</dd>
                <dt>Release</dt>
                <dd class="mono">{{ i.release ?? '—' }}</dd>
                <dt>Last reading</dt>
                <dd class="mono">
                  {{ i.reported_at }}
                  <span [class.error]="i.stale">({{ i.age_seconds }}s ago)</span>
                </dd>
              </dl>

              <table>
                <thead>
                  <tr><th>Subsystem</th><th>State</th><th>Detail</th></tr>
                </thead>
                <tbody>
                  @for (s of i.subsystems; track s.name) {
                    <tr [class.flag]="s.state !== 'ok'">
                      <td class="mono">{{ s.name }}</td>
                      <td><span [class]="pill(s.state)">{{ s.state }}</span></td>
                      <td>{{ s.detail }}</td>
                    </tr>
                  }
                </tbody>
              </table>
            </section>
          }
        }
      }
    </div>
  `,
})
export class SystemPage {
  private readonly api = inject(CollectorApi);
  data = signal<HealthResponse | null>(null);

  constructor() {
    this.load();
    // Liveness is only useful if it is current.
    setInterval(() => this.load(), 15_000);
  }

  load() {
    this.api.health().subscribe((d) => this.data.set(d));
  }

  pill(state: HealthState): string {
    switch (state) {
      case 'ok':
        return 'status good';
      case 'degraded':
        return 'status warn';
      default:
        return 'status bad';
    }
  }
}
